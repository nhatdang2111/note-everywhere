// Shows start-up failures instead of a blank window. Loaded as a classic
// script before the app bundle, so it still runs when the bundle cannot load
// (a blocked resource, a WebAssembly failure, an exception during start-up).
(function () {
  function show(message) {
    var root = document.getElementById("root");
    // Once the app has rendered, its own error handling takes over.
    if (!root || root.childElementCount > 0) return;
    var box = document.getElementById("boot-error");
    if (!box) {
      box = document.createElement("pre");
      box.id = "boot-error";
      box.style.cssText = "margin:24px;white-space:pre-wrap;font:13px/1.5 ui-monospace,monospace";
      box.textContent = "Note Everywhere could not start.\n";
      document.body.appendChild(box);
    }
    box.textContent += "\n" + message;
  }
  window.addEventListener("error", function (event) {
    show(String(event.message || event.error || "Unknown error"));
  });
  window.addEventListener("unhandledrejection", function (event) {
    show(String((event.reason && event.reason.message) || event.reason));
  });
  document.addEventListener("securitypolicyviolation", function (event) {
    show("Blocked by the content security policy (" + event.violatedDirective + "): " + event.blockedURI);
  });
})();
