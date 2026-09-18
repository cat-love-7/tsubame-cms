import { ComponentFixture, TestBed } from '@angular/core/testing';

import { ArrayField } from './array-field';

describe('ArrayField', () => {
  let component: ArrayField;
  let fixture: ComponentFixture<ArrayField>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [ArrayField],
    }).compileComponents();

    fixture = TestBed.createComponent(ArrayField);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
