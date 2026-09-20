// Tell crawlers to stay out of everything this distribution serves.
//
// This is the screen an editor signs in to and the API a site's build reads, not a public site, and
// an SPA answers 200 for every route it is asked about: without this a crawler indexes the shell
// under whatever URL it guessed. `robots.txt` is a request and the app's `noindex` meta only covers
// what the app renders, so the header is the layer that enforces it.
//
// A CloudFront function rather than a response headers policy: the same edge-side effect (cached
// responses included, so no invalidation is needed) and a resource the deploying identity is
// allowed to create - see `deployer-policy-edge.json`.
function handler(event) {
    var response = event.response;
    response.headers['x-robots-tag'] = { value: 'noindex, nofollow' };
    return response;
}
