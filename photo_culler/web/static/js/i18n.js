(() => {
  const picker = document.getElementById("language-picker");
  if (!picker) return;
  picker.addEventListener("change", () => {
    const url = new URL(window.location.href);
    url.searchParams.set("lang", picker.value);
    window.location.assign(url);
  });
})();
