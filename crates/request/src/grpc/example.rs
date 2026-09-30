use prost_reflect::{FieldDescriptor, Kind, MessageDescriptor};
use serde_json::{Map, Value, json};

/// Recursive messages stop here, so a self-referencing type stays finite.
const MAX_DEPTH: usize = 4;

/// An example JSON message using the proto3 JSON mapping. Each oneof shows
/// its first field; repeated fields and maps show one entry.
pub(super) fn message(descriptor: &MessageDescriptor) -> String {
    serde_json::to_string_pretty(&message_value(descriptor, 0)).unwrap_or_default()
}

fn message_value(descriptor: &MessageDescriptor, depth: usize) -> Value {
    if let Some(value) = well_known(descriptor) {
        return value;
    }

    let mut object = Map::new();

    if depth > MAX_DEPTH {
        return Value::Object(object);
    }

    for field in descriptor.fields() {
        if let Some(oneof) = field.containing_oneof()
            && !oneof.is_synthetic()
            && oneof.fields().next().is_some_and(|first| first != field)
        {
            continue;
        }

        object.insert(field.json_name().to_owned(), field_value(&field, depth));
    }

    Value::Object(object)
}

fn field_value(field: &FieldDescriptor, depth: usize) -> Value {
    if field.is_map() {
        let Kind::Message(entry) = field.kind() else {
            return json!({});
        };
        let key = match single_value(&entry.map_entry_key_field(), depth) {
            Value::String(key) => key,
            key => key.to_string(),
        };

        return json!({ key: single_value(&entry.map_entry_value_field(), depth) });
    }

    if field.is_list() {
        return json!([single_value(field, depth)]);
    }

    single_value(field, depth)
}

fn single_value(field: &FieldDescriptor, depth: usize) -> Value {
    match field.kind() {
        Kind::Double | Kind::Float => json!(1.5),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => json!(-1),
        Kind::Uint32 | Kind::Fixed32 => json!(1),
        // The JSON mapping writes 64-bit integers as strings.
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => json!("-1"),
        Kind::Uint64 | Kind::Fixed64 => json!("1"),
        Kind::Bool => json!(true),
        Kind::String => json!(field.name()),
        Kind::Bytes => json!("SGVsbG8="),
        Kind::Enum(descriptor) => descriptor
            .values()
            .next()
            .map_or(json!(0), |value| json!(value.name())),
        Kind::Message(descriptor) => message_value(&descriptor, depth + 1),
    }
}

/// Well-known types use special JSON forms instead of objects.
fn well_known(descriptor: &MessageDescriptor) -> Option<Value> {
    let value = match descriptor.full_name() {
        "google.protobuf.Timestamp" => json!("2025-01-01T00:00:00Z"),
        "google.protobuf.Duration" => json!("1s"),
        "google.protobuf.FieldMask" => json!("field"),
        "google.protobuf.Struct" => json!({}),
        "google.protobuf.ListValue" => json!([]),
        "google.protobuf.Value" => json!(null),
        "google.protobuf.Empty" => json!({}),
        "google.protobuf.Any" => json!({}),
        "google.protobuf.StringValue" => json!("value"),
        "google.protobuf.BytesValue" => json!("SGVsbG8="),
        "google.protobuf.BoolValue" => json!(true),
        "google.protobuf.DoubleValue" | "google.protobuf.FloatValue" => json!(1.5),
        "google.protobuf.Int32Value" => json!(-1),
        "google.protobuf.UInt32Value" => json!(1),
        "google.protobuf.Int64Value" => json!("-1"),
        "google.protobuf.UInt64Value" => json!("1"),
        _ => return None,
    };

    Some(value)
}
