// Live preview and arrow-button reordering for the layout form. Without this
// script the form still works: position numbers set the order, and the
// "Update preview" button loads the preview.
(function () {
  "use strict";

  var form = document.getElementById("layout-form");
  if (!form) return;

  var previewButton = document.getElementById("preview-button");
  var list = document.getElementById("home-sections");
  var timer = null;

  function refreshPreview() {
    clearTimeout(timer);
    timer = setTimeout(function () { form.requestSubmit(previewButton); }, 150);
  }

  function renumber() {
    var rows = list.querySelectorAll(".section-row");
    rows.forEach(function (row, index) {
      row.querySelector(".order-input").value = String(index + 1);
      row.querySelector(".order-up").disabled = index === 0;
      row.querySelector(".order-down").disabled = index === rows.length - 1;
    });
  }

  list.querySelectorAll(".order-input").forEach(function (input) { input.hidden = true; });
  list.querySelectorAll(".order-up, .order-down").forEach(function (button) { button.hidden = false; });
  renumber();

  list.addEventListener("click", function (event) {
    var button = event.target.closest(".order-up, .order-down");
    if (!button) return;
    var row = button.closest(".section-row");
    if (button.classList.contains("order-up") && row.previousElementSibling) {
      list.insertBefore(row, row.previousElementSibling);
    } else if (button.classList.contains("order-down") && row.nextElementSibling) {
      list.insertBefore(row.nextElementSibling, row);
    }
    renumber();
    button.focus();
    refreshPreview();
  });

  form.addEventListener("change", refreshPreview);
  refreshPreview();
})();
