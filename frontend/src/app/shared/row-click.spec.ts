import { clickedAControl } from './row-click';

/**
 * The rule the two list screens share: which clicks belong to a control inside the row rather than
 * to the row itself. The screens are what exercises it for real; this pins the rule down so a
 * selector that quietly stops matching - a class renamed on one of the cells, say - is caught here
 * rather than by a reader who opened an item on the way to its delete button.
 */
describe('clickedAControl', () => {
  const tree = document.createElement('div');
  tree.innerHTML = `
    <table><tbody><tr id="row">
      <td class="select"><input id="checkbox" type="checkbox"></td>
      <td id="value">text</td>
      <td class="actions">
        <button id="button"><span id="icon">edit</span></button>
        <a id="link" href="#x">name</a>
      </td>
    </tr></tbody></table>`;

  const at = (id: string) => tree.querySelector(`#${id}`);

  it('counts a click that landed on a control inside the row', () => {
    expect(clickedAControl(at('checkbox'))).toBe(true);
    expect(clickedAControl(at('button'))).toBe(true);
    // The icon of a button is still that button's click.
    expect(clickedAControl(at('icon'))).toBe(true);
    expect(clickedAControl(at('link'))).toBe(true);
    // The whole of a control's cell, so a click on its padding is not the row's either.
    expect(clickedAControl(at('checkbox')?.closest('td') ?? null)).toBe(true);
  });

  it('leaves a click on the row itself to the row', () => {
    expect(clickedAControl(at('value'))).toBe(false);
    expect(clickedAControl(at('row'))).toBe(false);
    // A target that is not an element - the document, a window - names no control.
    expect(clickedAControl(document)).toBe(false);
    expect(clickedAControl(null)).toBe(false);
  });
});
