'use strict';

// The test copies this site to a temporary directory and sets both variables, so the file does not
// have to know where either the plugin or the stub lives.
module.exports = {
  plugins: [
    {
      resolve: process.env.TSUBAME_PLUGIN_PATH,
      options: { apiUrl: process.env.TSUBAME_API_URL },
    },
  ],
};
