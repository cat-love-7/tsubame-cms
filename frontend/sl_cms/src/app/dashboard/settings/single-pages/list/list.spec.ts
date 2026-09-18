import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';

import { SinglePageSchemaList } from './list';

describe('SinglePageSchemaList', () => {
  let component: SinglePageSchemaList;
  let fixture: ComponentFixture<SinglePageSchemaList>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [SinglePageSchemaList],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    }).compileComponents();

    fixture = TestBed.createComponent(SinglePageSchemaList);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  it('refuses a blank page name instead of creating one', () => {
    component.newName = '   ';
    component.create();
    expect(component.error()).toEqual({ key: 'content.requiredPageName' });
  });
});
