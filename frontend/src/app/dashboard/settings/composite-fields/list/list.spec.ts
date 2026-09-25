import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';

import { CompositeFieldList } from './list';

describe('CompositeFieldList', () => {
  let component: CompositeFieldList;
  let fixture: ComponentFixture<CompositeFieldList>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [CompositeFieldList],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    }).compileComponents();

    fixture = TestBed.createComponent(CompositeFieldList);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  it('refuses a blank id instead of creating one', () => {
    component.newId = '  ';
    component.create();
    expect(component.error()).toEqual({ key: 'content.requiredCompositeId' });
  });
});
