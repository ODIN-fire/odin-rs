// script to load the theme css based on a "theme=..." query parameter of the current document
// note this needs to be in a script element of type text/javascript to make sure document.currentScript is defined

const searchParams = new URLSearchParams(window.location.search);
let theme = searchParams.get('theme');
let lnk = document.createElement('link');
lnk.id = "theme";
lnk.type ='text/css';
lnk.rel ='stylesheet';

switch (theme) {
  case 'light': lnk.href = './asset/odin_server/ui_theme_light.css'; break;
  case 'night': lnk.href = './asset/odin_server/ui_theme_night.css'; break;
  default: lnk.href = './asset/odin_server/ui_theme_dark.css'; break;
}

document.currentScript.insertAdjacentElement('afterend', lnk);
