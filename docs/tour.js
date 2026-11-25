const bar = document.querySelector(".tour-tabs");
const tabs = [...bar.querySelectorAll("a")];
const panels = tabs.map((tab) => document.querySelector(tab.hash));

function selectTab(selected) {
  tabs.forEach((tab, index) => {
    const active = tab === selected;
    tab.setAttribute("aria-selected", String(active));
    tab.tabIndex = active ? 0 : -1;
    panels[index].hidden = !active;
  });
}

bar.setAttribute("role", "tablist");
tabs.forEach((tab, index) => {
  tab.setAttribute("role", "tab");
  tab.setAttribute("aria-controls", panels[index].id);
  panels[index].setAttribute("role", "tabpanel");
  panels[index].setAttribute("aria-labelledby", tab.id);
  tab.addEventListener("click", (event) => {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    selectTab(tab);
    history.replaceState(null, "", tab.hash);
  });
  tab.addEventListener("keydown", (event) => {
    const next = {
      ArrowLeft: (index + tabs.length - 1) % tabs.length,
      ArrowRight: (index + 1) % tabs.length,
      Home: 0,
      End: tabs.length - 1,
    }[event.key];
    if (next === undefined) return;
    event.preventDefault();
    tabs[next].focus();
    tabs[next].click();
  });
});
function selectLinkedPanel() {
  selectTab(tabs.find((tab) => tab.hash === location.hash) || tabs[0]);
}
selectLinkedPanel();
window.addEventListener("hashchange", selectLinkedPanel);

const setupViewer = document.querySelector(".setup-viewer");
if (typeof setupViewer.showModal === "function") {
  const previews = [...document.querySelectorAll(".step-art")];
  const figure = setupViewer.querySelector(".viewer-figure");
  const image = figure.querySelector("img");
  let currentStep = 0;

  function showStep(index) {
    currentStep = (index + previews.length) % previews.length;
    const preview = previews[currentStep];
    const step = preview.closest(".setup-step");
    image.src = preview.href;
    image.alt = preview.querySelector("img").alt;
    figure.querySelector(".camera-choice")?.remove();
    const choice = preview.querySelector(".camera-choice");
    if (choice) figure.append(choice.cloneNode(true));
    setupViewer.querySelector("#viewer-title").textContent = step.querySelector("h3").textContent;
    setupViewer.querySelector("#viewer-description").textContent = step.querySelector("p").textContent;
    setupViewer.querySelector(".viewer-position").textContent =
      `Step ${currentStep + 1} of ${previews.length}`;
  }

  previews.forEach((preview, index) => {
    preview.setAttribute("aria-haspopup", "dialog");
    preview.addEventListener("click", (event) => {
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
      showStep(index);
      setupViewer.showModal();
    });
  });
  setupViewer.querySelectorAll("[data-offset]").forEach((button) => {
    button.addEventListener("click", () => showStep(currentStep + Number(button.dataset.offset)));
  });
  setupViewer.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    showStep(currentStep + (event.key === "ArrowLeft" ? -1 : 1));
  });
  setupViewer.addEventListener("click", (event) => {
    if (event.target !== setupViewer) return;
    const bounds = setupViewer.getBoundingClientRect();
    if (
      event.clientX < bounds.left ||
      event.clientX > bounds.right ||
      event.clientY < bounds.top ||
      event.clientY > bounds.bottom
    )
      setupViewer.close();
  });
}

// Sections are fully visible before enhancement, including when a visitor follows a deep link.
if ("IntersectionObserver" in window) {
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      entry.target.classList.add("in-view");
      observer.unobserve(entry.target);
    }
  });
  document
    .querySelectorAll(".section-head, .setup-heading, .setup-step, .download-row")
    .forEach((element) => observer.observe(element));
}
