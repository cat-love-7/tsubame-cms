// Send a path that looks like a screen to `index.html`.
//
// The app is one document with a router: `/collections/blog/edit/1` is a route, and the bucket has
// no object for it. A request for something with a file extension is a file and is served as
// asked. This runs on the distribution's default behavior only, so `/api/*` never reaches it - the
// API's own 403s and 404s are answered by the function, which is why the fallback is here rather
// than a distribution-wide custom error response (that would rewrite them too).
function handler(event) {
    var request = event.request;
    if (!request.uri.includes('.')) {
        request.uri = '/index.html';
    }
    return request;
}
