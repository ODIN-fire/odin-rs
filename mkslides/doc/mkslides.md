# mkslides

The `mkslides` crate is a tooling application that turns single [markdown](https://www.markdownguide.org/) files into stand alone
HTML slide decks. It is a tool to create presentations that can be easily shared online and do not require end user installation.

While `mkslides` does not strive to provide the bells and whistles of dedicated presentation applications it is efficient to 
show the flow of thought in a compact single text file, and to quickly create presentations with a consistent look and feel that
can be shown together with other web content (e.g. ODIN server pages).

The basic structure of an input markdown file looks like this:

```markdown
# ODIN Introduction

11/20/2025

<address>
somebody@odin-fire.org <br>
The ODIN Foundation <br>
</address>

---

## ODIN

  - ODIN = Open Data Integration Framework
  - software library to build servers for natural disaster mitigation
  ...

---

## Problem(s) to Solve

  - a lot of data is available but fragmented, not accessible to end users or not kept up-to-date
  - users likely end up with mix of browser tabs and applications ⇒ lack of integration
  - stakeholder orgs have to reinvent the wheel if they try to create & update (location/incident/role) specific apps

<br>
<img src="images/info-fragmentation.svg" class="center scale50">

---

...

---
```

The presentation title is the H1 (`# ODIN Introduction`), slides are separated by `---` patterns (with an empty line before and after)
and slide titles are the H2 (`##  Problem(s) to Solve`) headers preceding each slide content.

The slide contents are normally just markdown lists but can contain any valid markdown, including interspersed HTML elements should
more formatting or dynamic content be required. See [basic markdown syntax](https://www.markdownguide.org/basic-syntax/) for details.

One common pattern is to have slide content that combines a slide title, an item list and a diagram/image as in the example above. To
provide some additional formatting options (horizontal alignment, scaling) we normally use HTML `img` elements with automatically
defined classes (e.g. `center`, `left`, `right`, `scale10` .. `scale100`). To support scaling and reduce size it is recommended to use 
the [Scalable Vector Graphics (SVG)](https://en.wikipedia.org/wiki/SVG) format, which is supported by many readily available tools
(e.g. the open source [`InkScape`](https://inkscape.org/) project).

The usual directory structure follow this layout:

```
odin-intro/
    images/
        info-fragmentation.svg
        ...
    odin-intro.md                   # mkslides input file
    odin-intro.html                 # mkslides output file
```

To create the mkslides output execute the following command from within the presentation directory:

```
mkslides <options> odin-intro.md
```

Available options include formatting parameters and template/CSS paths to use. Please run with `--help` to see what is currently
supported.

To view presentations simple load the generated output file (or a link thereof) into your preferred browser. Basic navigation is
using the following hotkeys:

- pgDown / pgUp : transition to next / prev slide
- space : next slide
- `n` : toggle navigation menu (which is operated by mouse)
- `f` / esc : enter / exit fullscreen mode
- `t` : start / stop timer (showing 5sec increments of elapsed time in top right corner)



