// The app's routing: refuse the preview site, then send a path that looks like a screen to
// `index.html`.
//
// The preview site shares this bucket (its objects live under `preview/`), but it is served by its
// own distribution on its own name, and it must not be reachable here. This origin holds the
// editor's bearer token in `localStorage` and the app never renders CMS HTML; a preview renders a
// *draft*, which is exactly why the two are separate origins (`docs/preview-site.md` §7). Without
// this guard the bucket would serve the preview's script from this origin, so the isolation would
// rest only on the preview app never rendering anything for a path it does not recognise - which is
// not a boundary, it is a coincidence.
//
// A refusal rather than a rewrite: rewriting to `/index.html` would answer 200 with the admin app
// under a `/preview/...` URL, which is the same confusion in a quieter form.
//
// This is a function of its own rather than a branch in `spa-routing.js` because the preview
// distribution uses that one to serve its own routes, which all begin with `/preview/` - the same
// path this one has to refuse. One function cannot do both, and CloudFront allows one viewer-request
// association per cache behavior.
function handler(event) {
    var request = event.request;
    var uri = request.uri;

    if (uri === '/preview' || uri.indexOf('/preview/') === 0) {
        return {
            statusCode: 404,
            statusDescription: 'Not Found',
            headers: { 'content-type': { value: 'text/plain; charset=utf-8' } },
            body: 'Not Found',
        };
    }

    // A path with no file extension in it is a route, not a file: the app is one document. The
    // API's own 403s and 404s are answered by the function, which is why the fallback is here rather
    // than a distribution-wide custom error response (that would rewrite them too).
    if (!uri.includes('.')) {
        request.uri = '/index.html';
    }
    return request;
}
