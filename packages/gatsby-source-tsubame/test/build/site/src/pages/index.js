import * as React from 'react';
import { graphql } from 'gatsby';

/**
 * The one page of the build test.
 *
 * The query is the assertion: a union field cannot be queried without naming a member, so if the
 * plugin declared the wrong type - or a plain list of one node type - this fails to compile, and if
 * `resolveType` or the field resolver is wrong it fails or answers null at runtime. The build test
 * reads the page data back and compares it with what the stub served.
 */
export default function Index({ data }) {
  const [item] = data.allTsubameBlogItem.nodes;
  return <main>{item.title}</main>;
}

export const query = graphql`
  query {
    allTsubameBlogItem {
      nodes {
        title
        values
        related {
          ... on TsubameAuthorsItem {
            __typename
            name
          }
          ... on TsubameEditorsItem {
            __typename
            name
          }
          ... on TsubameRelationRef {
            __typename
            target
            item
            kind
          }
        }
        mentions {
          ... on TsubameAuthorsItem {
            __typename
            name
          }
          ... on TsubameRelationRef {
            __typename
            target
            item
            kind
          }
        }
        card {
          label
          related {
            ... on TsubameAuthorsItem {
              __typename
              name
            }
            ... on TsubameEditorsItem {
              __typename
              name
            }
          }
        }
      }
    }
  }
`;
