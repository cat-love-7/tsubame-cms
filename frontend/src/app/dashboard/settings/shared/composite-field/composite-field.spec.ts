import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';

import { CompositeField } from './composite-field';

describe('CompositeField', () => {
  let component: CompositeField;
  let fixture: ComponentFixture<CompositeField>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [CompositeField],
      providers: [provideHttpClient(), provideHttpClientTesting()],
    }).compileComponents();

    fixture = TestBed.createComponent(CompositeField);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
