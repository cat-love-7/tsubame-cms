import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { provideRouter } from '@angular/router';

import { CollectionSchemaCreate } from './create';

describe('CollectionSchemaCreate', () => {
  let component: CollectionSchemaCreate;
  let fixture: ComponentFixture<CollectionSchemaCreate>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [CollectionSchemaCreate],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    }).compileComponents();

    fixture = TestBed.createComponent(CollectionSchemaCreate);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
