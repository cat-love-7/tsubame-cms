import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { provideRouter } from '@angular/router';

import { CollectionSchemaList } from './list';

describe('CollectionSchemaList', () => {
  let component: CollectionSchemaList;
  let fixture: ComponentFixture<CollectionSchemaList>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [CollectionSchemaList],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    }).compileComponents();

    fixture = TestBed.createComponent(CollectionSchemaList);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
