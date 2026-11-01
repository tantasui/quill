// Article editor: formatting toolbar, live preview, image uploads, local time
// zones and an unsaved-changes guard. The form still submits without it.
(function () {
  "use strict";

  var form = document.getElementById("editor-form");
  if (!form) return;

  var text = document.getElementById("content");
  var panes = form.querySelector(".writer-panes");
  var preview = document.getElementById("preview");
  var saveState = document.getElementById("save-state");
  var csrf = form.getAttribute("data-csrf");
  var dirty = false;

  // ---- Unsaved changes -------------------------------------------------------
  function markDirty() {
    if (!dirty) {
      dirty = true;
      saveState.textContent = "Unsaved changes";
    }
  }
  form.addEventListener("input", markDirty);
  form.addEventListener("submit", function (event) {
    var submitter = event.submitter;
    if (!submitter || !submitter.hasAttribute("data-no-dirty-reset")) dirty = false;
  });
  window.addEventListener("beforeunload", function (event) {
    if (dirty) { event.preventDefault(); event.returnValue = ""; }
  });

  // ---- Title: a textarea so long titles wrap, but it behaves like one line --------
  var title = document.getElementById("title");
  function fitTitle() {
    title.style.height = "auto";
    title.style.height = title.scrollHeight + "px";
  }
  title.addEventListener("keydown", function (event) {
    if (event.key === "Enter") { event.preventDefault(); text.focus(); }
  });
  title.addEventListener("input", function () {
    if (/[\r\n]/.test(title.value)) title.value = title.value.replace(/[\r\n]+/g, " ");
    fitTitle();
  });
  window.addEventListener("resize", fitTitle);
  fitTitle();

  // ---- Time zones: show and submit publication dates in local time ---------------
  var dateInput = document.getElementById("published_at");
  var tzInput = document.getElementById("tz_offset");
  function pad(n) { return String(n).padStart(2, "0"); }
  function toLocalInput(date) {
    return date.getFullYear() + "-" + pad(date.getMonth() + 1) + "-" + pad(date.getDate()) +
      "T" + pad(date.getHours()) + ":" + pad(date.getMinutes());
  }
  var utc = dateInput.getAttribute("data-utc");
  if (utc && tzInput.value === "0") {
    var parsed = new Date(utc + ":00Z");
    if (!isNaN(parsed)) dateInput.value = toLocalInput(parsed);
  }
  function updateOffset() {
    var when = dateInput.value ? new Date(dateInput.value) : new Date();
    tzInput.value = String(isNaN(when) ? new Date().getTimezoneOffset() : when.getTimezoneOffset());
  }
  updateOffset();
  dateInput.addEventListener("change", updateOffset);

  // ---- Formatting toolbar ----------------------------------------------------------
  function replaceSelection(before, after, placeholder) {
    var start = text.selectionStart, end = text.selectionEnd;
    var selected = text.value.slice(start, end) || placeholder || "";
    text.focus();
    text.setRangeText(before + selected + after, start, end, "end");
    if (!text.value.slice(start, end).length || selected === placeholder) {
      text.setSelectionRange(start + before.length, start + before.length + selected.length);
    }
    text.dispatchEvent(new Event("input", { bubbles: true }));
  }

  function prefixLines(prefixFn) {
    var value = text.value;
    var start = value.lastIndexOf("\n", text.selectionStart - 1) + 1;
    var endIndex = value.indexOf("\n", text.selectionEnd);
    var end = endIndex === -1 ? value.length : endIndex;
    var lines = value.slice(start, end).split("\n");
    var result = lines.map(function (line, i) { return prefixFn(i) + line; }).join("\n");
    text.focus();
    text.setRangeText(result, start, end, "select");
    text.dispatchEvent(new Event("input", { bubbles: true }));
  }

  var actions = {
    bold: function () { replaceSelection("**", "**", "bold text"); },
    italic: function () { replaceSelection("*", "*", "italic text"); },
    heading: function () { prefixLines(function () { return "## "; }); },
    quote: function () { prefixLines(function () { return "> "; }); },
    ul: function () { prefixLines(function () { return "- "; }); },
    ol: function () { prefixLines(function (i) { return (i + 1) + ". "; }); },
    code: function () {
      var selected = text.value.slice(text.selectionStart, text.selectionEnd);
      if (selected.indexOf("\n") !== -1) replaceSelection("```\n", "\n```", "");
      else replaceSelection("`", "`", "code");
    },
    link: function () {
      var url = window.prompt("Link address", "https://");
      if (url) replaceSelection("[", "](" + url + ")", "link text");
    },
    image: function () { document.getElementById("image-file").click(); }
  };

  form.querySelector(".writer-tools").addEventListener("click", function (event) {
    var button = event.target.closest("[data-md]");
    if (button) actions[button.getAttribute("data-md")]();
  });

  text.addEventListener("keydown", function (event) {
    if (!(event.ctrlKey || event.metaKey)) return;
    var key = event.key.toLowerCase();
    if (key === "b") { event.preventDefault(); actions.bold(); }
    else if (key === "i") { event.preventDefault(); actions.italic(); }
    else if (key === "k") { event.preventDefault(); actions.link(); }
  });

  // Continue lists when pressing Enter.
  text.addEventListener("keydown", function (event) {
    if (event.key !== "Enter" || event.shiftKey || event.isComposing) return;
    var value = text.value, pos = text.selectionStart;
    var lineStart = value.lastIndexOf("\n", pos - 1) + 1;
    var line = value.slice(lineStart, pos);
    var match = line.match(/^(\s*)([-*+]|\d+\.)\s+(\[[ x]\]\s+)?/);
    if (!match) return;
    event.preventDefault();
    if (line.trim() === match[0].trim()) {
      text.setRangeText("", lineStart, pos, "end"); // empty item ends the list
    } else {
      var marker = /\d+\./.test(match[2]) ? (parseInt(match[2], 10) + 1) + "." : match[2];
      text.setRangeText("\n" + match[1] + marker + " " + (match[3] ? "[ ] " : ""), pos, pos, "end");
    }
    text.dispatchEvent(new Event("input", { bubbles: true }));
  });

  document.addEventListener("keydown", function (event) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      var save = form.querySelector("[data-shortcut-save]");
      if (save) form.requestSubmit(save);
    }
  });

  // ---- Live preview ------------------------------------------------------------------
  var previewTimer = null, previewSeq = 0;
  function refreshPreview() {
    if (panes.getAttribute("data-mode") === "write") return;
    var seq = ++previewSeq;
    var body = new URLSearchParams({ _csrf: csrf, content: text.value });
    fetch(form.getAttribute("data-preview-url"), { method: "POST", body: body, credentials: "same-origin" })
      .then(function (r) { if (!r.ok) throw new Error(r.status); return r.text(); })
      .then(function (html) {
        if (seq !== previewSeq) return;
        preview.innerHTML = html.trim() ? html : '<p class="empty-preview">Nothing to preview yet.</p>';
      })
      .catch(function () {
        preview.innerHTML = '<p class="empty-preview">Preview unavailable. Your session may have expired; save to keep your work.</p>';
      });
  }
  text.addEventListener("input", function () {
    clearTimeout(previewTimer);
    previewTimer = setTimeout(refreshPreview, 300);
  });

  form.querySelector(".writer-modes").addEventListener("click", function (event) {
    var button = event.target.closest("[data-mode]");
    if (!button) return;
    var mode = button.getAttribute("data-mode");
    panes.setAttribute("data-mode", mode);
    form.querySelectorAll(".writer-modes [data-mode]").forEach(function (b) {
      b.setAttribute("aria-selected", String(b === button));
    });
    try { localStorage.setItem("quill-editor-mode", mode); } catch (e) { /* ignore */ }
    refreshPreview();
  });
  try {
    var saved = localStorage.getItem("quill-editor-mode");
    if (saved && saved !== "write" && !(saved === "split" && window.innerWidth < 832)) {
      var btn = form.querySelector('.writer-modes [data-mode="' + saved + '"]');
      if (btn) btn.click();
    }
  } catch (e) { /* ignore */ }

  // ---- Uploads -----------------------------------------------------------------------
  function upload(file) {
    var data = new FormData();
    data.append("_csrf", csrf);
    data.append("file", file);
    saveState.textContent = "Uploading " + file.name + "…";
    return fetch(form.getAttribute("data-upload-url"), {
      method: "POST", body: data, credentials: "same-origin", headers: { Accept: "application/json" }
    }).then(function (r) {
      return r.json().catch(function () { throw new Error("Upload failed. Your session may have expired."); })
        .then(function (json) {
          if (!r.ok) throw new Error(json.error || "Upload failed.");
          saveState.textContent = dirty ? "Unsaved changes" : "";
          return json;
        });
    }).catch(function (err) {
      saveState.textContent = "";
      window.alert(err.message);
      throw err;
    });
  }

  function insertImage(file) {
    upload(file).then(function (res) {
      var alt = file.name.replace(/\.[^.]+$/, "").replace(/[-_]+/g, " ");
      var start = text.selectionStart;
      var needsBreak = start > 0 && text.value[start - 1] !== "\n";
      text.focus();
      text.setRangeText((needsBreak ? "\n\n" : "") + "![" + alt + "](" + res.reference + ")\n", start, text.selectionEnd, "end");
      text.dispatchEvent(new Event("input", { bubbles: true }));
    }, function () {});
  }

  document.getElementById("image-file").addEventListener("change", function () {
    if (this.files[0]) insertImage(this.files[0]);
    this.value = "";
  });

  text.addEventListener("paste", function (event) {
    var files = event.clipboardData && event.clipboardData.files;
    if (files && files.length && files[0].type.indexOf("image/") === 0) {
      event.preventDefault();
      insertImage(files[0]);
    }
  });
  panes.addEventListener("dragover", function (event) {
    if (event.dataTransfer.types.indexOf("Files") !== -1) { event.preventDefault(); panes.classList.add("is-dragging"); }
  });
  panes.addEventListener("dragleave", function () { panes.classList.remove("is-dragging"); });
  panes.addEventListener("drop", function (event) {
    panes.classList.remove("is-dragging");
    var file = event.dataTransfer.files[0];
    if (file) { event.preventDefault(); insertImage(file); }
  });

  // Cover image
  var cover = document.getElementById("cover_image");
  var coverPreview = document.getElementById("cover-preview");
  var coverClear = document.getElementById("cover-clear");
  var coverFile = document.getElementById("cover-file");
  var base = cover.getAttribute("data-base");
  function showCover() {
    var value = cover.value.trim();
    coverPreview.hidden = !value;
    coverClear.hidden = !value;
    if (value) coverPreview.src = value.indexOf("/media/") === 0 ? base + value : value;
  }
  cover.addEventListener("change", showCover);
  coverClear.addEventListener("click", function () { cover.value = ""; showCover(); markDirty(); });
  document.getElementById("cover-upload").addEventListener("click", function () { coverFile.click(); });
  coverFile.addEventListener("change", function () {
    var file = this.files[0];
    this.value = "";
    if (file) upload(file).then(function (res) { cover.value = res.reference; showCover(); markDirty(); }, function () {});
  });

  // ---- Character counters for search fields -----------------------------------------------
  form.querySelectorAll("[data-count]").forEach(function (field) {
    var limit = parseInt(field.getAttribute("data-count"), 10);
    var counter = document.createElement("span");
    counter.className = "char-count";
    counter.setAttribute("aria-live", "polite");
    field.insertAdjacentElement("afterend", counter);
    function update() {
      var n = field.value.length;
      counter.textContent = n + " / " + limit + (n > limit ? " — may be shortened in search results" : "");
      counter.classList.toggle("over", n > limit);
    }
    field.addEventListener("input", update);
    update();
  });
})();
