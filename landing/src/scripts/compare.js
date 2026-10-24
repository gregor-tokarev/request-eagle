// Styles keep the navigation bar opaque until this script is running to track the scroll.
document.documentElement.classList.add("js");

const nav = document.querySelector("[data-nav]");

function followScroll() {
  nav.classList.toggle("is-scrolled", window.scrollY > 24);
}

window.addEventListener("scroll", followScroll, { passive: true });
followScroll();
