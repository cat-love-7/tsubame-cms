import { ParamMap, convertToParamMap } from '@angular/router';
import { BehaviorSubject } from 'rxjs';

/**
 * A stub `ActivatedRoute` whose parameters a test can change.
 *
 * Screens read their parameters from `paramMap` and `queryParamMap` rather than from `snapshot`,
 * because the router reuses a component when only the parameter changes: switching pages in the
 * sidebar has to reload the screen, and a stub carrying a snapshot alone could not show that it
 * does. A query parameter is a parameter too - a second reset link is the same route with another
 * token - so it gets a stream of its own.
 */
export interface StubActivatedRoute {
  paramMap: BehaviorSubject<ParamMap>;
  queryParamMap: BehaviorSubject<ParamMap>;
  /** Navigate to another resource, as the sidebar does. */
  navigate(params: Record<string, string>): void;
  /** Change the query string without leaving the screen, as opening another link on it does. */
  navigateQuery(query: Record<string, string>): void;
}

export function stubActivatedRoute(
  params: Record<string, string>,
  query: Record<string, string> = {},
): StubActivatedRoute {
  const paramMap = new BehaviorSubject<ParamMap>(convertToParamMap(params));
  const queryParamMap = new BehaviorSubject<ParamMap>(convertToParamMap(query));
  return {
    paramMap,
    queryParamMap,
    navigate(next: Record<string, string>) {
      paramMap.next(convertToParamMap(next));
    },
    navigateQuery(next: Record<string, string>) {
      queryParamMap.next(convertToParamMap(next));
    },
  };
}
