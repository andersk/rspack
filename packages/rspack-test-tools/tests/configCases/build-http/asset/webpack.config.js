const path = require("path");

/** @type {import("@rspack/core").Configuration} */
module.exports = {
	mode: "development",
	experiments: {
		buildHttp: {
			allowedUris: ["https://"],
			lockfileLocation: path.resolve(__dirname, "./lock-files/lock.json"),
			cacheLocation: path.resolve(__dirname, "./lock-files/test")
		},
		css: false
	}
};
