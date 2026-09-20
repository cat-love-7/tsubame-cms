//! The rules a relation is published under.
//!
//! One rule, in both directions (see `doc/relations-design.md` §4): what the site serves must not
//! have a required relation that points at nothing published. Publishing is checked against the
//! targets the copy names; unpublishing is checked against the published content that names it -
//! the same question, asked from the other end.
//!
//! An optional relation is free to point at a draft: the delivery API drops what is not published
//! (see `http::content`), and refusing here would take away the freedom to publish in any order a
//! site needs.
//!
//! Both rules read [`ContentReader`] rather than a repository of their own, because "the other
//! end" may be an item or a page whichever way the reference goes.

use std::collections::HashMap;

use crate::models::error::{HttpError, map_internal_error};
use crate::models::owner::ItemOwner;
use crate::models::schema::{CompositeFieldId, CompositeFieldSchema, FieldSchema};
use crate::models::values::{FieldValueMap, relation_values};
use crate::repositories::content_reader::ContentReader;
use crate::repositories::relation_repository::RelationRepository;

/// Whether every required relation a copy holds points at something the site serves.
///
/// Called where the copy goes live, with the values that are about to be published.
pub async fn ensure_required_relations_are_published(
    schema: &[FieldSchema],
    values: &FieldValueMap<Vec<FieldSchema>>,
    composites: &HashMap<CompositeFieldId, CompositeFieldSchema>,
    reader: &dyn ContentReader,
) -> Result<(), HttpError> {
    for relation in relation_values(schema, values, composites) {
        // A required field that holds *nothing* is `validate_to_schema`'s refusal, and it says
        // something better about it ("the field is missing" rather than "the target is a draft").
        if !relation.required || relation.references.is_empty() {
            continue;
        }
        let mut served = false;
        for reference in relation.references {
            if is_published(&reference.target_owner(), reader).await? {
                served = true;
                break;
            }
        }
        if !served {
            let target = relation.references[0].target_owner();
            return Err(HttpError::relation_unpublished(&relation.path, &target));
        }
    }
    Ok(())
}

/// Whether taking `target` off the site would leave published content with nothing to point at.
///
/// Called before the target is unpublished, with the index answering who names it. A referrer that
/// is not on the site asks nothing of it, and one that holds another published reference in the
/// same field is served by that reference instead.
pub async fn ensure_unpublish_keeps_required_referrers(
    target: &ItemOwner,
    composites: &HashMap<CompositeFieldId, CompositeFieldSchema>,
    reader: &dyn ContentReader,
    relations: &dyn RelationRepository,
) -> Result<(), HttpError> {
    let referrers = relations
        .get_relation_references(target)
        .await
        .map_err(map_internal_error)?;
    for referrer in referrers {
        let Some(content) = reader
            .read_content(&referrer)
            .await
            .map_err(map_internal_error)?
        else {
            continue;
        };
        if !content.published {
            continue;
        }
        let Some(values) = content.published_values.as_ref() else {
            continue;
        };
        for relation in relation_values(&content.schema, values, composites) {
            if !relation.required
                || !relation
                    .references
                    .iter()
                    .any(|reference| &reference.target_owner() == target)
            {
                continue;
            }
            let mut served = false;
            for reference in relation.references {
                let owner = reference.target_owner();
                if &owner == target {
                    continue;
                }
                if is_published(&owner, reader).await? {
                    served = true;
                    break;
                }
            }
            if !served {
                return Err(HttpError::relation_required_by(&referrer, &relation.path));
            }
        }
    }
    Ok(())
}

/// Whether the content a reference names is on the site.
///
/// Gone counts as not published: the rule is about what the site serves, and a reference to
/// something deleted serves nothing.
async fn is_published(owner: &ItemOwner, reader: &dyn ContentReader) -> Result<bool, HttpError> {
    Ok(reader
        .read_content(owner)
        .await
        .map_err(map_internal_error)?
        .map(|content| content.published)
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::marker::PhantomData;

    use super::*;
    use crate::models::values::{
        CompositeFieldReference, CompositeFieldValue, FieldType, FieldValue, RelationOptions,
        RelationRef, RelationTarget, TextFieldOptions,
    };
    use crate::repositories::content_reader::{OwnedContent, ReadContentFuture};
    use crate::repositories::relation_repository::RelationReferencesFuture;

    /// A site of content: what exists, and whether it is published.
    #[derive(Default)]
    struct FakeContent(
        HashMap<ItemOwner, (Vec<FieldSchema>, bool, FieldValueMap<Vec<FieldSchema>>)>,
    );

    impl FakeContent {
        fn published(
            mut self,
            owner: ItemOwner,
            schema: Vec<FieldSchema>,
            values: FieldValueMap<Vec<FieldSchema>>,
        ) -> Self {
            self.0.insert(owner, (schema, true, values));
            self
        }

        fn draft(
            mut self,
            owner: ItemOwner,
            schema: Vec<FieldSchema>,
            values: FieldValueMap<Vec<FieldSchema>>,
        ) -> Self {
            self.0.insert(owner, (schema, false, values));
            self
        }
    }

    impl ContentReader for FakeContent {
        fn read_content(&self, owner: &ItemOwner) -> ReadContentFuture<'_> {
            let content = self
                .0
                .get(owner)
                .map(|(schema, published, values)| OwnedContent {
                    schema: schema.clone(),
                    published: *published,
                    published_values: Some(values.clone()),
                    working_values: None,
                });
            Box::pin(async move { Ok(content) })
        }
    }

    /// An index that answers with a fixed set of referrers.
    struct FakeRelations(Vec<ItemOwner>);

    impl RelationRepository for FakeRelations {
        fn get_relation_references(&self, _target: &ItemOwner) -> RelationReferencesFuture<'_> {
            let referrers = self.0.clone();
            Box::pin(async move { Ok(referrers) })
        }

        fn detach_references(
            &self,
            _target: &ItemOwner,
        ) -> crate::repositories::relation_repository::DetachFuture<'_> {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    fn relation_field(name: &str, target: &str, required: bool, has_many: bool) -> FieldSchema {
        FieldSchema {
            is_title: false,
            show_in_list: false,
            name: name.to_string(),
            field_type: FieldType::Relation(RelationOptions {
                target: RelationTarget::Collection {
                    name: target.to_string(),
                },
                has_many,
                inverse_name: None,
            }),
            required,
            width: 12,
            height: 1,
            unique: false,
        }
    }

    fn text_field(name: &str) -> FieldSchema {
        FieldSchema {
            is_title: false,
            show_in_list: false,
            name: name.to_string(),
            field_type: FieldType::Text(TextFieldOptions::default()),
            required: false,
            width: 12,
            height: 1,
            unique: false,
        }
    }

    fn references(items: &[(u64, bool)]) -> FieldValueMap<Vec<FieldSchema>> {
        let refs: Vec<RelationRef> = items
            .iter()
            .map(|(item, _)| RelationRef {
                target: "authors".to_string(),
                item: Some(*item),
            })
            .collect();
        FieldValueMap(
            HashMap::from([("author".to_string(), FieldValue::Relation(refs))]),
            PhantomData,
        )
    }

    fn values(entries: Vec<(&str, FieldValue)>) -> FieldValueMap<Vec<FieldSchema>> {
        FieldValueMap(
            entries
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
            PhantomData,
        )
    }

    /// A site where author 1 is published and author 2 is a draft.
    fn authors() -> FakeContent {
        FakeContent::default()
            .published(
                ItemOwner::collection_item("authors", 1),
                vec![text_field("title")],
                values(vec![("title", FieldValue::Text("Ada".into()))]),
            )
            .draft(
                ItemOwner::collection_item("authors", 2),
                vec![text_field("title")],
                values(vec![("title", FieldValue::Text("Grace".into()))]),
            )
    }

    #[tokio::test]
    async fn a_required_relation_has_to_point_at_something_published() {
        let schema = vec![relation_field("author", "authors", true, false)];
        let reader = authors();

        // The only target is a draft: refused, and the refusal names the field and the target.
        let error = ensure_required_relations_are_published(
            &schema,
            &references(&[(2, false)]),
            &HashMap::new(),
            &reader,
        )
        .await
        .unwrap_err();
        assert_eq!(error.status_code, 409);
        assert_eq!(error.code, "relation_unpublished");
        assert_eq!(error.field.as_deref(), Some("author"));
        assert!(
            error.message.contains("authors item 2"),
            "{}",
            error.message
        );

        // One published target is enough, wherever in the set it sits.
        assert!(
            ensure_required_relations_are_published(
                &schema,
                &references(&[(2, false), (1, true)]),
                &HashMap::new(),
                &reader,
            )
            .await
            .is_ok()
        );

        // A target that is gone is not published either.
        let error = ensure_required_relations_are_published(
            &schema,
            &references(&[(99, false)]),
            &HashMap::new(),
            &reader,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "relation_unpublished");
    }

    #[tokio::test]
    async fn an_optional_relation_may_point_at_a_draft() {
        let schema = vec![relation_field("author", "authors", false, false)];
        assert!(
            ensure_required_relations_are_published(
                &schema,
                &references(&[(2, false)]),
                &HashMap::new(),
                &authors(),
            )
            .await
            .is_ok(),
            "the delivery API drops it instead"
        );
    }

    #[tokio::test]
    async fn an_empty_required_relation_is_left_to_the_schema_check() {
        // Nothing held is "the field is missing", which has a better message than "the target is
        // a draft" - and `validate_to_schema` asks it before this rule runs.
        let schema = vec![relation_field("author", "authors", true, true)];
        assert!(
            ensure_required_relations_are_published(
                &schema,
                &references(&[]),
                &HashMap::new(),
                &authors(),
            )
            .await
            .is_ok()
        );
    }

    #[tokio::test]
    async fn a_required_relation_inside_a_composite_is_checked_too() {
        let composite_id = CompositeFieldId::from("cta");
        let mut composites = HashMap::new();
        composites.insert(
            composite_id.clone(),
            vec![relation_field("author", "authors", true, false)],
        );
        let schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "cta".to_string(),
            field_type: FieldType::CompositeField(CompositeFieldReference { id: composite_id }),
            required: false,
            width: 12,
            height: 1,
            unique: false,
        }];
        let nested = values(vec![(
            "cta",
            FieldValue::CompositeField(Some(CompositeFieldValue {
                id: CompositeFieldId::from("cta"),
                values: references(&[(2, false)]),
            })),
        )]);

        let error =
            ensure_required_relations_are_published(&schema, &nested, &composites, &authors())
                .await
                .unwrap_err();
        assert_eq!(error.code, "relation_unpublished");
        assert_eq!(
            error.field.as_deref(),
            Some("cta.author"),
            "the path says which input to open"
        );
    }

    #[tokio::test]
    async fn a_required_relation_inside_an_array_element_names_its_index() {
        let composite_id = CompositeFieldId::from("block");
        let mut composites = HashMap::new();
        composites.insert(
            composite_id.clone(),
            vec![relation_field("author", "authors", true, false)],
        );
        let schema = vec![FieldSchema {
            is_title: false,
            show_in_list: false,
            name: "blocks".to_string(),
            field_type: FieldType::Array(vec![FieldType::CompositeField(
                CompositeFieldReference {
                    id: composite_id.clone(),
                },
            )]),
            required: false,
            width: 12,
            height: 1,
            unique: false,
        }];
        let element = |item: u64| {
            FieldValue::CompositeField(Some(CompositeFieldValue {
                id: composite_id.clone(),
                values: references(&[(item, false)]),
            }))
        };
        let nested = values(vec![(
            "blocks",
            FieldValue::Array(vec![
                FieldValue::CompositeField(None),
                element(2),
                element(1),
            ]),
        )]);

        let error =
            ensure_required_relations_are_published(&schema, &nested, &composites, &authors())
                .await
                .unwrap_err();
        assert_eq!(error.field.as_deref(), Some("blocks[1].author"));
    }

    #[tokio::test]
    async fn unpublished_referrers_do_not_hold_a_target_on_the_site() {
        let schema = vec![relation_field("author", "authors", true, false)];
        let content = FakeContent::default().draft(
            ItemOwner::collection_item("posts", 1),
            schema,
            references(&[(1, true)]),
        );
        let relations = FakeRelations(vec![ItemOwner::collection_item("posts", 1)]);

        // The referrer is not on the site, so taking the author down asks nothing of it.
        assert!(
            ensure_unpublish_keeps_required_referrers(
                &ItemOwner::collection_item("authors", 1),
                &HashMap::new(),
                &content,
                &relations,
            )
            .await
            .is_ok()
        );
    }

    #[tokio::test]
    async fn a_published_referrer_keeps_a_target_it_requires() {
        let schema = vec![relation_field("author", "authors", true, false)];
        let content = FakeContent::default().published(
            ItemOwner::collection_item("posts", 1),
            schema.clone(),
            references(&[(1, true)]),
        );
        let relations = FakeRelations(vec![ItemOwner::collection_item("posts", 1)]);

        let error = ensure_unpublish_keeps_required_referrers(
            &ItemOwner::collection_item("authors", 1),
            &HashMap::new(),
            &content,
            &relations,
        )
        .await
        .unwrap_err();
        assert_eq!(error.status_code, 409);
        assert_eq!(error.code, "relation_required_by");
        assert!(error.message.contains("posts item 1"), "{}", error.message);

        // Another published reference in the same field is what the site serves instead.
        let elsewhere = FakeContent::default()
            .published(
                ItemOwner::collection_item("posts", 1),
                schema,
                references(&[(1, true), (3, true)]),
            )
            .published(
                ItemOwner::collection_item("authors", 3),
                vec![text_field("title")],
                values(vec![("title", FieldValue::Text("Other".into()))]),
            );
        assert!(
            ensure_unpublish_keeps_required_referrers(
                &ItemOwner::collection_item("authors", 1),
                &HashMap::new(),
                &elsewhere,
                &relations,
            )
            .await
            .is_ok()
        );
    }

    #[tokio::test]
    async fn an_optional_relation_does_not_hold_a_target_on_the_site() {
        let schema = vec![relation_field("author", "authors", false, false)];
        let content = FakeContent::default().published(
            ItemOwner::collection_item("posts", 1),
            schema,
            references(&[(1, true)]),
        );
        let relations = FakeRelations(vec![ItemOwner::collection_item("posts", 1)]);

        assert!(
            ensure_unpublish_keeps_required_referrers(
                &ItemOwner::collection_item("authors", 1),
                &HashMap::new(),
                &content,
                &relations,
            )
            .await
            .is_ok(),
            "an optional relation is dropped from delivery instead"
        );
    }
}
