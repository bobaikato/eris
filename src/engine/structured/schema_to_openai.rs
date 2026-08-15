//! Lower a `schemars` Draft-7 [`RootSchema`] into the JSON-Schema subset accepted by OpenAI
//! strict structured outputs. Analogous to [`crate::engine::grammar::schema_to_gbnf_rule`].
//!
//! A typed, closed DTO ([`OpenAiSchema`]) models *only* the accepted subset and renders exactly
//! the right JSON, so the invariants (`additionalProperties: false`, all properties `required`,
//! optionals expressed as nullable unions) are unrepresentable-to-violate rather than enforced by
//! hand. Unsupported constructs return `Err`; the per-tool caller falls back to a permissive
//! empty-object schema (same graceful-degradation philosophy as `schema_to_gbnf`).

use schemars::schema::{
    ArrayValidation, InstanceType, ObjectValidation, RootSchema, Schema, SchemaObject,
    SingleOrVec,
};
use serde_json::{json, Value};

use crate::executive::error::{FcpError, Result};

/// Closed model of the OpenAI-strict JSON-Schema subset.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenAiSchema {
    /// Always renders `additionalProperties: false` with **every** property listed in `required`
    /// (strict-mode invariant). Optional Rust fields are lowered as [`Self::Nullable`].
    Object {
        /// Sorted by key for deterministic output (mirrors the GBNF compiler).
        properties: Vec<(String, OpenAiSchema)>,
    },
    String {
        enum_values: Option<Vec<String>>,
    },
    Integer,
    Number,
    Boolean,
    Null,
    Array {
        items: Box<OpenAiSchema>,
    },
    /// Renders as `anyOf: [inner, {"type": "null"}]` — how strict mode expresses optionality.
    Nullable(Box<OpenAiSchema>),
}

impl OpenAiSchema {
    /// Permissive fallback for tools whose schema cannot be lowered: an empty object
    /// (strict mode forbids free-form objects, so "no constrained args" is the safe shape).
    #[must_use]
    pub fn empty_object() -> Self {
        Self::Object {
            properties: Vec::new(),
        }
    }

    /// Render to the exact JSON accepted by `response_format: json_schema (strict)`.
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::Object { properties } => {
                let mut props = serde_json::Map::new();
                let mut required: Vec<Value> = Vec::with_capacity(properties.len());
                for (key, schema) in properties {
                    props.insert(key.clone(), schema.to_value());
                    required.push(Value::String(key.clone()));
                }
                json!({
                    "type": "object",
                    "properties": Value::Object(props),
                    "required": required,
                    "additionalProperties": false,
                })
            }
            Self::String { enum_values } => match enum_values {
                Some(values) => json!({ "type": "string", "enum": values }),
                None => json!({ "type": "string" }),
            },
            Self::Integer => json!({ "type": "integer" }),
            Self::Number => json!({ "type": "number" }),
            Self::Boolean => json!({ "type": "boolean" }),
            Self::Null => json!({ "type": "null" }),
            Self::Array { items } => json!({ "type": "array", "items": items.to_value() }),
            Self::Nullable(inner) => json!({ "anyOf": [inner.to_value(), { "type": "null" }] }),
        }
    }
}

const MAX_DEPTH: u8 = 8;

struct LowerCtx<'a> {
    definitions: &'a schemars::Map<String, Schema>,
    depth: u8,
}

fn unsupported(what: &str) -> FcpError {
    FcpError::SchemaViolation(format!("schema_to_openai: unsupported construct: {what}"))
}

/// Lower a tool's full [`RootSchema`] (inlining `#/definitions/…` refs).
pub fn lower_root_schema(schema: &RootSchema) -> Result<OpenAiSchema> {
    let mut ctx = LowerCtx {
        definitions: &schema.definitions,
        depth: 0,
    };
    lower_schema_object(&schema.schema, &mut ctx)
}

/// Per-tool entry point with graceful degradation: falls back to [`OpenAiSchema::empty_object`]
/// (logging the reason) when the schema cannot be lowered. Because tool args are typed Rust
/// structs deriving `JsonSchema`, most tools lower cleanly.
#[must_use]
pub fn tool_args_schema(tool_name: &str, schema: &RootSchema) -> OpenAiSchema {
    match lower_root_schema(schema) {
        Ok(lowered) => lowered,
        Err(e) => {
            tracing::warn!(
                tool = tool_name,
                error = %e,
                "schema_to_openai: falling back to permissive empty-object args schema"
            );
            OpenAiSchema::empty_object()
        }
    }
}

fn lower_schema_object(schema: &SchemaObject, ctx: &mut LowerCtx<'_>) -> Result<OpenAiSchema> {
    if let Some(subs) = schema.subschemas.as_ref()
        && (subs.one_of.is_some() || subs.any_of.is_some() || subs.all_of.is_some())
    {
        return Err(unsupported("oneOf/anyOf/allOf subschema"));
    }

    if let Some(ref reference) = schema.reference {
        return resolve_ref(reference, ctx);
    }

    let instance_type = match &schema.instance_type {
        Some(SingleOrVec::Single(t)) => Some(**t),
        Some(SingleOrVec::Vec(types)) => {
            if types.len() == 2 && types.contains(&InstanceType::Null) {
                let non_null = types
                    .iter()
                    .find(|t| **t != InstanceType::Null)
                    .ok_or_else(|| unsupported("type: [null, null]"))?;
                let mut inner = schema.clone();
                inner.instance_type = Some(SingleOrVec::Single(Box::new(*non_null)));
                return Ok(OpenAiSchema::Nullable(Box::new(lower_schema_object(
                    &inner, ctx,
                )?)));
            }
            return Err(unsupported("multi-type (non-nullable) union"));
        }
        None => None,
    };

    match instance_type {
        Some(InstanceType::Object) => lower_object(schema.object.as_deref(), ctx),
        Some(InstanceType::String) => Ok(lower_string(schema)),
        Some(InstanceType::Integer) => Ok(OpenAiSchema::Integer),
        Some(InstanceType::Number) => Ok(OpenAiSchema::Number),
        Some(InstanceType::Boolean) => Ok(OpenAiSchema::Boolean),
        Some(InstanceType::Null) => Ok(OpenAiSchema::Null),
        Some(InstanceType::Array) => lower_array(schema.array.as_deref(), ctx),
        None => {
            if schema.enum_values.is_some() {
                Ok(lower_string(schema))
            } else if schema.object.is_some() {
                lower_object(schema.object.as_deref(), ctx)
            } else {
                Err(unsupported("schema with no instance_type and no recognizable shape"))
            }
        }
    }
}

fn resolve_ref(reference: &str, ctx: &mut LowerCtx<'_>) -> Result<OpenAiSchema> {
    let def_name = reference
        .strip_prefix("#/definitions/")
        .ok_or_else(|| unsupported("non-local $ref"))?;
    let definition = ctx
        .definitions
        .get(def_name)
        .ok_or_else(|| unsupported("unresolved $ref"))?;
    if ctx.depth >= MAX_DEPTH {
        return Err(unsupported("nesting/$ref depth exceeded"));
    }
    ctx.depth += 1;
    let out = match definition {
        Schema::Object(obj) => lower_schema_object(obj, ctx),
        Schema::Bool(_) => Err(unsupported("boolean schema definition")),
    };
    ctx.depth -= 1;
    out
}

fn lower_string(schema: &SchemaObject) -> OpenAiSchema {
    let enum_values = schema.enum_values.as_ref().and_then(|values| {
        let strings: Vec<String> = values
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        (!strings.is_empty()).then_some(strings)
    });
    OpenAiSchema::String { enum_values }
}

fn lower_object(
    validation: Option<&ObjectValidation>,
    ctx: &mut LowerCtx<'_>,
) -> Result<OpenAiSchema> {
    let Some(validation) = validation else {
        return Ok(OpenAiSchema::empty_object());
    };

    let free_form_additional = validation
        .additional_properties
        .as_deref()
        .is_some_and(|s| !matches!(s, Schema::Bool(false)));
    if free_form_additional && validation.properties.is_empty() {
        return Err(unsupported("free-form additionalProperties"));
    }

    if ctx.depth >= MAX_DEPTH {
        return Err(unsupported("nesting depth exceeded"));
    }
    ctx.depth += 1;

    let required: std::collections::HashSet<&str> =
        validation.required.iter().map(String::as_str).collect();

    let mut keys: Vec<&String> = validation.properties.keys().collect();
    keys.sort();

    let mut properties: Vec<(String, OpenAiSchema)> = Vec::with_capacity(keys.len());
    let mut result: Result<()> = Ok(());
    for key in keys {
        let prop = match validation.properties.get(key) {
            Some(Schema::Object(obj)) => lower_schema_object(obj, ctx),
            Some(Schema::Bool(true)) => Err(unsupported("any-typed property")),
            Some(Schema::Bool(false)) | None => Err(unsupported("false/missing property schema")),
        };
        match prop {
            Ok(lowered) => {
                // Strict mode lists every property in `required`; a schemars-optional field
                // becomes nullable so the model can express "absent" as null.
                let lowered = if required.contains(key.as_str())
                    || matches!(lowered, OpenAiSchema::Nullable(_))
                {
                    lowered
                } else {
                    OpenAiSchema::Nullable(Box::new(lowered))
                };
                properties.push((key.clone(), lowered));
            }
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }

    ctx.depth -= 1;
    result?;
    Ok(OpenAiSchema::Object { properties })
}

fn lower_array(
    validation: Option<&ArrayValidation>,
    ctx: &mut LowerCtx<'_>,
) -> Result<OpenAiSchema> {
    let items = match validation.and_then(|v| v.items.as_ref()) {
        Some(SingleOrVec::Single(item_schema)) => match item_schema.as_ref() {
            Schema::Object(obj) => lower_schema_object(obj, ctx)?,
            Schema::Bool(_) => return Err(unsupported("boolean array item schema")),
        },
        Some(SingleOrVec::Vec(_)) => return Err(unsupported("tuple-typed array items")),
        None => return Err(unsupported("array without item schema")),
    };
    Ok(OpenAiSchema::Array {
        items: Box::new(items),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::Deserialize;

    fn lower_for<T: JsonSchema>() -> Result<OpenAiSchema> {
        lower_root_schema(&schemars::schema_for!(T))
    }

    fn value_for<T: JsonSchema>() -> Value {
        lower_for::<T>().expect("lower").to_value()
    }

    #[derive(JsonSchema, Deserialize)]
    #[allow(dead_code)]
    struct SimpleRequired {
        relative_path: String,
    }

    #[test]
    fn simple_required_string_field() {
        let v = value_for::<SimpleRequired>();
        assert_eq!(v["type"], "object");
        assert_eq!(v["additionalProperties"], false);
        assert_eq!(v["properties"]["relative_path"]["type"], "string");
        assert_eq!(v["required"], json!(["relative_path"]));
    }

    #[derive(JsonSchema, Deserialize)]
    #[allow(dead_code)]
    struct WithOptional {
        required_field: String,
        optional_field: Option<String>,
    }

    #[test]
    fn optional_field_becomes_nullable_but_stays_required() {
        let v = value_for::<WithOptional>();
        let required: Vec<&str> = v["required"]
            .as_array()
            .expect("required array")
            .iter()
            .filter_map(|x| x.as_str())
            .collect();
        assert!(required.contains(&"required_field"));
        assert!(
            required.contains(&"optional_field"),
            "strict mode lists every property in required"
        );
        let opt = &v["properties"]["optional_field"];
        let any_of = opt["anyOf"].as_array().expect("anyOf union for optional");
        assert!(any_of.iter().any(|s| s["type"] == "null"));
        assert!(any_of.iter().any(|s| s["type"] == "string"));
    }

    #[derive(JsonSchema, Deserialize)]
    #[allow(dead_code)]
    #[serde(rename_all = "lowercase")]
    enum TestMode {
        Overwrite,
        Append,
    }

    #[derive(JsonSchema, Deserialize)]
    #[allow(dead_code)]
    struct WithEnum {
        mode: TestMode,
    }

    #[test]
    fn enum_field_inlines_definitions_ref() {
        let v = value_for::<WithEnum>();
        let mode = &v["properties"]["mode"];
        assert_eq!(mode["type"], "string");
        assert_eq!(mode["enum"], json!(["overwrite", "append"]));
        assert!(
            v.to_string().find("$ref").is_none(),
            "refs must be inlined: {v}"
        );
    }

    #[derive(JsonSchema, Deserialize)]
    #[allow(dead_code)]
    struct MixedScalars {
        minutes: u32,
        ratio: f64,
        permanent: bool,
        tags: Vec<String>,
    }

    #[test]
    fn scalar_and_array_types() {
        let v = value_for::<MixedScalars>();
        assert_eq!(v["properties"]["minutes"]["type"], "integer");
        assert_eq!(v["properties"]["ratio"]["type"], "number");
        assert_eq!(v["properties"]["permanent"]["type"], "boolean");
        assert_eq!(v["properties"]["tags"]["type"], "array");
        assert_eq!(v["properties"]["tags"]["items"]["type"], "string");
    }

    #[derive(JsonSchema, Deserialize)]
    #[allow(dead_code)]
    struct NestedOptions {
        timeout: u32,
        verbose: bool,
    }

    #[derive(JsonSchema, Deserialize)]
    #[allow(dead_code)]
    struct NestedArgs {
        label: String,
        options: NestedOptions,
    }

    #[test]
    fn nested_object_all_levels_closed() {
        let v = value_for::<NestedArgs>();
        let options = &v["properties"]["options"];
        assert_eq!(options["type"], "object");
        assert_eq!(options["additionalProperties"], false);
        assert_eq!(options["properties"]["timeout"]["type"], "integer");
    }

    #[derive(JsonSchema, Deserialize)]
    #[allow(dead_code)]
    struct EmptyArgs {}

    #[test]
    fn empty_args_is_closed_empty_object() {
        let v = value_for::<EmptyArgs>();
        assert_eq!(v["type"], "object");
        assert_eq!(v["additionalProperties"], false);
        assert_eq!(v["required"], json!([]));
    }

    #[test]
    fn unsupported_oneof_errors_and_tool_fallback_is_empty_object() {
        use schemars::schema::SubschemaValidation;
        let mut root = schemars::schema_for!(EmptyArgs);
        root.schema.subschemas = Some(Box::new(SubschemaValidation {
            one_of: Some(vec![Schema::Bool(true)]),
            ..Default::default()
        }));
        assert!(lower_root_schema(&root).is_err());
        let fallback = tool_args_schema("test:unsupported", &root);
        assert_eq!(fallback, OpenAiSchema::empty_object());
    }

    #[derive(JsonSchema, Deserialize)]
    #[allow(dead_code)]
    struct FreeFormArgs {
        payload: serde_json::Value,
    }

    #[test]
    fn free_form_value_field_is_unsupported() {
        assert!(lower_for::<FreeFormArgs>().is_err());
    }

    #[test]
    fn properties_sorted_deterministically() {
        #[derive(JsonSchema, Deserialize)]
        #[allow(dead_code)]
        struct Unsorted {
            zebra: String,
            alpha: String,
        }
        let v = value_for::<Unsorted>();
        let keys: Vec<&String> = v["properties"]
            .as_object()
            .expect("object")
            .keys()
            .collect();
        assert_eq!(keys, vec!["alpha", "zebra"]);
    }
}
