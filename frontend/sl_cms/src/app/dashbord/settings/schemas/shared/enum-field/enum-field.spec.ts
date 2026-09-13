import { ComponentFixture, TestBed } from '@angular/core/testing';

import { EnumField } from './enum-field';

describe('EnumField', () => {
  let component: EnumField;
  let fixture: ComponentFixture<EnumField>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [EnumField]
    })
    .compileComponents();

    fixture = TestBed.createComponent(EnumField);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
