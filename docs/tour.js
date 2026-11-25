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
