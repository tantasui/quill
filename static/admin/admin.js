// Small progressive enhancements for the dashboard. Every page works without them.
(function () {
  "use strict";

  // Confirm destructive actions: <button data-confirm="Are you sure?">
  document.addEventListener("click", function (event) {
    var button = event.target.closest("[data-confirm]");
    if (button && !window.confirm(button.getAttribute("data-confirm"))) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  }, true);

  function flash(button, label) {
    var original = button.textContent;
    button.textContent = label;
    setTimeout(function () { button.textContent = original; }, 1500);
  }

  function copy(text, button) {
    if (navigator.clipboard && window.isSecureContext) {
      navigator.clipboard.writeText(text).then(function () { flash(button, "Copied"); });
    } else {
      var area = document.createElement("textarea");
      area.value = text;
      document.body.appendChild(area);
      area.select();
      try { document.execCommand("copy"); flash(button, "Copied"); } catch (e) { /* ignore */ }
      area.remove();
    }
  }

  // <button data-copy="#element-id"> or <button data-copy-text="literal">
  document.addEventListener("click", function (event) {
    var button = event.target.closest("[data-copy], [data-copy-text]");
    if (!button) return;
    var text = button.getAttribute("data-copy-text");
    if (text === null) {
      var source = document.querySelector(button.getAttribute("data-copy"));
      text = source ? source.textContent : "";
    }
    copy(text, button);
  });

  // Drag-and-drop onto the media upload form.
  var zone = document.getElementById("media-upload");
  if (zone) {
    var input = zone.querySelector('input[type="file"]');
    ["dragenter", "dragover"].forEach(function (name) {
      zone.addEventListener(name, function (e) { e.preventDefault(); zone.classList.add("is-over"); });
    });
    ["dragleave", "drop"].forEach(function (name) {
      zone.addEventListener(name, function () { zone.classList.remove("is-over"); });
    });
    zone.addEventListener("drop", function (e) {
      e.preventDefault();
      if (e.dataTransfer.files.length) {
        input.files = e.dataTransfer.files;
        zone.submit();
      }
    });
    input.addEventListener("change", function () { if (input.files.length) zone.submit(); });
  }
})();
