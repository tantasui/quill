// Updates the design preview as soon as a control changes. Without this script
// the "Show" button does the same job.
(function () {
  "use strict";

  var form = document.getElementById("design-preview-form");
  if (!form) return;
  document.getElementById("preview-show").hidden = true;
  form.addEventListener("change", function () { form.submit(); });
})();
