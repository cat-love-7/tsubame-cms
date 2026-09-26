/**
 * A row that opens its item, from the browser's side.
 *
 * The Collection list and the Single page list both open an item when its row is clicked, and both
 * have cells that carry a control of their own: the selection checkbox, the buttons at the end of
 * the row, the name link of a page. Such a click belongs to that control, and the row's own
 * navigation must not happen on the way to it - a reader who came to publish something would
 * otherwise lose the list they were working in. One rule for the two screens, so a click means the
 * same thing wherever it lands.
 */
export function clickedAControl(target: EventTarget | null): boolean {
  return (
    target instanceof Element && target.closest('a, button, input, td.select, td.actions') !== null
  );
}
