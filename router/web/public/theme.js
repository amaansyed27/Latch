try {
  const preference = localStorage.getItem("latch-theme") || "system";
  document.documentElement.dataset.theme =
    preference === "system"
      ? matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : preference;
} catch {
  /* System default is provided by CSS. */
}
