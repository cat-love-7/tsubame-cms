// Send a path that looks like a screen to `index.html`.
//
// The site is one document with a router: `/preview/collections/blog/items/7` is a route, and the
// bucket has no object for it. A request for something with a file extension is a file and is served
// as asked.
//
// This is the **preview** distribution's routing (`infra/preview.tf`); the app has its own
// (`app-routing.js`), which also refuses `/preview/*` so that a preview cannot be served from the
// app's origin. This one must not do that: every preview route begins with `/preview/`.
function handler(event) {
    var request = event.request;
    if (!request.uri.includes('.')) {
        request.uri = '/index.html';
    }
    return request;
}
