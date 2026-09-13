import { ComponentFixture, TestBed } from '@angular/core/testing';

import { EditSchema } from './edit-schema';

describe('EditSchema', () => {
  let component: EditSchema;
  let fixture: ComponentFixture<EditSchema>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [EditSchema]
    })
    .compileComponents();

    fixture = TestBed.createComponent(EditSchema);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
