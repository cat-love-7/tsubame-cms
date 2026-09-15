import { ParamMap, convertToParamMap } from '@angular/router';
import { BehaviorSubject } from 'rxjs';

/**
 * A stub `ActivatedRoute` whose parameters a test can change.
 *
 * Screens read their parameters from `paramMap` rather than from `snapshot`, because the router
 * reuses a component when only the parameter changes: switching pages in the sidebar has to
 * reload the screen, and a stub carrying a snapshot alone could not show that it does.
 */
export interface StubActivatedRoute {
  paramMap: BehaviorSubject<ParamMap>;
  /** Navigate to another resource, as the sidebar does. */
  navigate(params: Record<string, string>): void;
}

export function stubActivatedRoute(params: Record<string, string>): StubActivatedRoute {
  const paramMap = new BehaviorSubject<ParamMap>(convertToParamMap(params));
  return {
    paramMap,
    navigate(next: Record<string, string>) {
      paramMap.next(convertToParamMap(next));
    },
  };
}
