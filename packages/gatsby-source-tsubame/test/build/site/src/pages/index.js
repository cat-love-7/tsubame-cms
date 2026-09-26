import * as React from 'react';
import { graphql } from 'gatsby';

/**
 * The one page of the build test.
 *
 * The query is half the assertion. A union field cannot be queried without naming a member, so if
 * the plugin declared the wrong type this fails to compile; `ghost` names a target the CMS does not
 * answer, so if the plugin had declared it (rather than leaving it out) this page could query it -
 * and the test asserts against the generated schema that it is not there.
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
        }
        mentions {
          __typename
          name
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
