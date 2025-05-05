// Populate the sidebar
//
// This is a script, and not included directly in the page, to control the total size of the book.
// The TOC contains an entry for each page, so if each page includes a copy of the TOC,
// the total size of the page becomes O(n**2).
class MDBookSidebarScrollbox extends HTMLElement {
    constructor() {
        super();
    }
    connectedCallback() {
        this.innerHTML = '<ol class="chapter"><li class="chapter-item expanded "><a href="about.html">About this Document</a></li><li class="chapter-item expanded "><a href="install.html">Installation</a></li><li class="chapter-item expanded "><a href="intro.html">1. Introduction</a><a class="toggle"><div>❱</div></a></li><li><ol class="section"><li class="chapter-item "><a href="design_principles.html">Design Principles</a></li></ol></li><li class="chapter-item expanded "><a href="sys_crates.html">2. System Crates</a><a class="toggle"><div>❱</div></a></li><li><ol class="section"><li class="chapter-item "><a href="odin_build/odin_build.html">odin_build</a></li><li class="chapter-item "><a href="odin_action/odin_action.html">odin_action</a></li><li class="chapter-item "><a href="odin_actor/odin_actor.html">odin_actor</a><a class="toggle"><div>❱</div></a></li><li><ol class="section"><li class="chapter-item "><a href="odin_actor/actor_basics.html">Actor Programming Model</a></li><li class="chapter-item "><a href="odin_actor/actor_impl.html">Basic Design</a></li><li class="chapter-item "><a href="odin_actor/actor_communication.html">Actor Communication</a></li><li class="chapter-item "><a href="odin_actor/examples/examples.html">Examples</a><a class="toggle"><div>❱</div></a></li><li><ol class="section"><li class="chapter-item "><a href="odin_actor/examples/hello_world.html">hello_world</a></li><li class="chapter-item "><a href="odin_actor/examples/sys_msgs.html">sys_msgs</a></li><li class="chapter-item "><a href="odin_actor/examples/spawn.html">spawn</a></li><li class="chapter-item "><a href="odin_actor/examples/spawn_blocking.html">spawn_blocking</a></li><li class="chapter-item "><a href="odin_actor/examples/exec.html">exec</a></li><li class="chapter-item "><a href="odin_actor/examples/jobs.html">jobs</a></li><li class="chapter-item "><a href="odin_actor/examples/producer_consumer.html">producer_consumer</a></li><li class="chapter-item "><a href="odin_actor/examples/pub_sub.html">pub_sub</a></li><li class="chapter-item "><a href="odin_actor/examples/pin_pong.html">ping_pong</a></li><li class="chapter-item "><a href="odin_actor/examples/query.html">query</a></li><li class="chapter-item "><a href="odin_actor/examples/dyn_actor.html">dyn_actor</a></li><li class="chapter-item "><a href="odin_actor/examples/actions.html">actions</a></li><li class="chapter-item "><a href="odin_actor/examples/dyn_actions.html">dyn_actions</a></li><li class="chapter-item "><a href="odin_actor/examples/retry.html">retry</a></li><li class="chapter-item "><a href="odin_actor/examples/requests.html">requests</a></li><li class="chapter-item "><a href="odin_actor/examples/actor_config.html">actor_config</a></li><li class="chapter-item "><a href="odin_actor/examples/heartbeat.html">heartbeat</a></li></ol></li></ol></li><li class="chapter-item "><a href="odin_server/odin_server.html">odin_server</a><a class="toggle"><div>❱</div></a></li><li><ol class="section"><li class="chapter-item "><a href="odin_server/client.html">Server/Client Interaction</a></li><li class="chapter-item "><a href="odin_server/ui_library.html">Web Client UI Library</a></li></ol></li><li class="chapter-item "><a href="odin_cesium/odin_cesium.html">odin_cesium</a></li><li class="chapter-item "><a href="odin_share/odin_share.html">odin_share</a></li><li class="chapter-item "><a href="odin_dem/odin_dem.html">odin-dem</a></li><li class="chapter-item "><a href="odin_gdal/odin_gdal.html">odin-gdal</a></li></ol></li><li class="chapter-item expanded "><a href="app_crates.html">3. Application Domain Crates</a><a class="toggle"><div>❱</div></a></li><li><ol class="section"><li class="chapter-item "><a href="odin_geolayer/odin_geolayer.html">odin_geolayer</a></li><li class="chapter-item "><a href="odin_hrrr/odin_hrrr.html">odin_hrrr</a></li><li class="chapter-item "><a href="odin_sentinel/odin_sentinel.html">odin_sentinel</a></li><li class="chapter-item "><a href="odin_goesr/odin_goesr.html">odin_goesr</a></li><li class="chapter-item "><a href="odin_orbital/odin_orbital.html">odin_orbital</a></li></ol></li><li class="chapter-item expanded "><a href="glossary.html">Glossary</a></li></ol>';
        // Set the current, active page, and reveal it if it's hidden
        let current_page = document.location.href.toString().split("#")[0];
        if (current_page.endsWith("/")) {
            current_page += "index.html";
        }
        var links = Array.prototype.slice.call(this.querySelectorAll("a"));
        var l = links.length;
        for (var i = 0; i < l; ++i) {
            var link = links[i];
            var href = link.getAttribute("href");
            if (href && !href.startsWith("#") && !/^(?:[a-z+]+:)?\/\//.test(href)) {
                link.href = path_to_root + href;
            }
            // The "index" page is supposed to alias the first chapter in the book.
            if (link.href === current_page || (i === 0 && path_to_root === "" && current_page.endsWith("/index.html"))) {
                link.classList.add("active");
                var parent = link.parentElement;
                if (parent && parent.classList.contains("chapter-item")) {
                    parent.classList.add("expanded");
                }
                while (parent) {
                    if (parent.tagName === "LI" && parent.previousElementSibling) {
                        if (parent.previousElementSibling.classList.contains("chapter-item")) {
                            parent.previousElementSibling.classList.add("expanded");
                        }
                    }
                    parent = parent.parentElement;
                }
            }
        }
        // Track and set sidebar scroll position
        this.addEventListener('click', function(e) {
            if (e.target.tagName === 'A') {
                sessionStorage.setItem('sidebar-scroll', this.scrollTop);
            }
        }, { passive: true });
        var sidebarScrollTop = sessionStorage.getItem('sidebar-scroll');
        sessionStorage.removeItem('sidebar-scroll');
        if (sidebarScrollTop) {
            // preserve sidebar scroll position when navigating via links within sidebar
            this.scrollTop = sidebarScrollTop;
        } else {
            // scroll sidebar to current active section when navigating via "next/previous chapter" buttons
            var activeSection = document.querySelector('#sidebar .active');
            if (activeSection) {
                activeSection.scrollIntoView({ block: 'center' });
            }
        }
        // Toggle buttons
        var sidebarAnchorToggles = document.querySelectorAll('#sidebar a.toggle');
        function toggleSection(ev) {
            ev.currentTarget.parentElement.classList.toggle('expanded');
        }
        Array.from(sidebarAnchorToggles).forEach(function (el) {
            el.addEventListener('click', toggleSection);
        });
    }
}
window.customElements.define("mdbook-sidebar-scrollbox", MDBookSidebarScrollbox);
