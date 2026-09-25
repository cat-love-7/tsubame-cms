import { ComponentFixture } from '@angular/core/testing';

/**
 * A fixture whose host element is an element.
 *
 * Angular declares `ComponentFixture.nativeElement` as `any`, so every `querySelector` on it is
 * untyped - a spec's DOM reads are unchecked, and a lint that reads the types has to report each
 * one. The host of a component fixture *is* an element, and `any` is assignable to `HTMLElement`,
 * so saying so in the declaration is enough: no assertion at the call site, and a query's result
 * (`Element | null`, `NodeListOf<Element>`) is typed from there on.
 */
export interface TypedFixture<T> extends ComponentFixture<T> {
  readonly nativeElement: HTMLElement;
}
