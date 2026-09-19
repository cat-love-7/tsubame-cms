use crate::models::identity::{StringId, UintId};
use crate::models::values::{FieldSchema, FieldValueMap, FieldValueResponse};
use std::collections::HashMap;

pub type CollectionSchema = Vec<FieldSchema>;

pub type CollectionItem = FieldValueMap<CollectionSchema>;
pub type CollectionItemResponse = HashMap<String, FieldValueResponse>;
pub type CollectionName = StringId<CollectionSchema>;

pub type CollectionItemId = UintId<CollectionSchema>;

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, marker::PhantomData};

    use super::*;
    use crate::models::values::{FieldType, FieldValue, TextFieldOptions};

    fn create_test_schema() -> CollectionSchema {
        vec![
            FieldSchema {
                is_title: false,
                show_in_list: false,
                name: "title".to_string(),
                field_type: FieldType::Text(TextFieldOptions::default()),
                required: true,
                width: 12,
                height: 1,
                unique: false,
            },
            FieldSchema {
                is_title: false,
                show_in_list: false,
                name: "tags".to_string(),
                field_type: FieldType::TextEnum(vec![
                    "news".to_string(),
                    "blog".to_string(),
                    "tutorial".to_string(),
                ]),
                required: false,
                width: 12,
                height: 1,
                unique: false,
            },
        ]
    }

    fn create_test_item() -> CollectionItem {
        FieldValueMap(
            HashMap::from([
                (
                    "title".to_string(),
                    FieldValue::Text("Hello World".to_string()),
                ),
                (
                    "tags".to_string(),
                    FieldValue::TextEnum(vec!["news".to_string(), "blog".to_string()]),
                ),
            ]),
            PhantomData,
        )
    }

    #[test]
    fn collection_schema_serialization() {
        let schema = create_test_schema();

        let serialized = serde_json::to_string(&schema).unwrap();
        assert_eq!(
            serialized,
            r#"[{"name":"title","field_type":{"Text":{"max_length":null,"min_length":null}},"required":true,"width":12,"height":1},{"name":"tags","field_type":{"TextEnum":["news","blog","tutorial"]},"required":false,"width":12,"height":1}]"#
        );

        let deserialized: CollectionSchema = serde_json::from_str(&serialized).unwrap();
        assert_eq!(schema, deserialized);
    }

    #[test]
    fn collection_item_serialization() {
        let item = create_test_item();

        let serialized = serde_json::to_string(&item).unwrap();
        let deserialized: CollectionItem = serde_json::from_str(&serialized).unwrap();
        assert_eq!(item, deserialized);
    }

    #[test]
    fn collection_item_deserialization_from_json() {
        let json = r#"{"tags":{"TextEnum":["news","blog"]},"title":{"Text":"Hello World"}}"#;
        let deserialized: CollectionItem = serde_json::from_str(&json).unwrap();

        let expected = create_test_item();
        assert_eq!(deserialized, expected);
    }
}
