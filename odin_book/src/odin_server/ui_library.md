# ODIN Web Client User Interface Library

`odin-rs` comes with its own web client user interface library that provides the common UI wdgets (`Window`, `List`, `CheckBox` etc.).
The basic reasons why we do not use available 3rd party libraries is laid out in [design principles](../design_principles.md) but
there are also application specific ones, namely:

(a) UI components have to be compact. Normally UI libraries are for rendering full web pages but in ODIN web applications the main
information is the geospatial display (virtual globe) that forms the background of the page. UI elements should not distract from
this and have to support moving them around. Our use case is much closer to a traditional desktop user interface (inside a browser
page) than it is to normal web page design.

(b) UI widgets have to support structured, dynamic data. The main use of UI components in `odin-rs` web applications is to display
layer specific alphanumeric data that can be dynamically updated through a websocket, and to control the CesiumJS / WebGL
rendering of selected data items. If widgets only work with basic, generic types such as strings and numbers it would require
a large amount of glue/adapter code in respective JS modules of such layers, which would be especially error prone in the context
of layer specific data items that are asynchronously updated at a high rate (e.g. for object tracking). This is particularly addressed
by our `List` widget. 

(c) Theme support. We not only want to support a wide range of display sizes/resolutions and OS platforms (possibly with native UI 
resemblance). Since we also target in-field applications there needs to be the end-user capability to choose between day/night displays
and high/low contrast modes. This requires extensive configuration support both on the server and locally on clients.


The `ui.js` module implements our library as a `odin_server` [asset](../odin_build/odin_build.md). It uses a fairly straight-forward
[DOM](https://developer.mozilla.org/en-US/docs/Web/API/Document_Object_Model) manipulation through Javascript in which each of its
components is represented by a [`DIV`](https://developer.mozilla.org/en-US/docs/Web/HTML/Element/div) element with `odin-rs` specific
class names such as `ui_window` or `ui_list`. The UI layout could be specified in a plain [HTML](https://developer.mozilla.org/en-US/docs/Web/HTML) document (if it has a post-load script that calls `initializeWindow(e)`) but this is not the usual case.
To keep (structural) layout, UI state and respective functions in one place we recommend using the layer specific JS modules for
all these aspects. This means UI components do not appear in the static HTML document source but are dynamically added when respective
JS modules get initialized.

The `ui.js` module uses the`ui.css` [CSS style sheet](https://developer.mozilla.org/en-US/docs/Web/CSS) for basic layout of its widgets.
The - rarely changed - `ui.css` stylesheet in turn uses a configured theme CSS (e.g. `ui_theme_dark.css`) that solely consists of a
(user modifiable) extensive set of [CSS custom properties](https://developer.mozilla.org/en-US/docs/Web/CSS) for colors, font-families, font sizes and other theme related styles. The `odin_cesium.js` UI does include a settings window that lets users choose between different
theme CSS files, and also supports modifying/storing/restoring respective CSS properties in browser local storage.

## Widgets

`odin_server/assets/ui.js` provides the following UI components that are implemented as [`HTMLElements](https://developer.mozilla.org/en-US/docs/Web/API/HTMLElement)


#### `Window`

The `Window` widget is one of the toplevel components provided by `ui.js`. Each interactive `odin-rs` service usually has one or more
layer/service specific windows that are defined in their JS module through a call of the `Window(title,id,icon)(components..)`
function like so:

```javascript
import * as ui from "../odin_server/ui.js";
...
function createWindow() {
    return ui.Window("GOES-R Satellites", "goesr", "./asset/odin_goesr/geo-sat-icon.svg")(
        ui.Panel("data sets", true)(
            ui.RowContainer()(
                ui.CheckBox("lock step", toggleGoesrLockStep, "goesr.lockStep"),
                ...
                ui.HorizontalSpacer(2),
                ui.CheckBox("G16", toggleShowGoesrSatellite, "goesr.G16"),
                ...
            ),
            ui.List("goesr.dataSets", 6, selectGoesrDataSet),
            ...
        ),
        ui.Panel("hotspots", true)(
            ...
        ),
        ...
    );
}
```

Note that the first argument group of the curried `Window(..)(..)` function defines the titlebar and the second argument group 
specifies the structured content and layout of a `Window` instance.

Although this is not required `Window` content components are usually organized into separate `Panels` so that respective content can be
expanded/collapsed on demand in order to minimize UI screen space.

`Windows` can be interactively moved through dragging their titlebar and shown/hidden through their associated icon (and titlebar buttons).

Normally, `Window` instances are static, i.e. they are defined and instantiated in the module initialization code, but only shown after
user interaction (e.g. clicking on the icon). They also can be created/disposed dynamically, which happens for per-data-item views
(e.g. the `ImageWindow`) or dialogs (e.g. to enter geospatial data such as polygons).

Creating, showing, hiding and disposing `Windows` is normally done through standard behavior triggered by user interaction, i.e. JS modules
rarely use functions other than the `Window(..)(..)` constructor mentioned above. There are no `Window` specific callbacks that
can or have to be provided.


#### `List`

The `List` widget and its derivate `TreeList` are perhaps the most important components in the `ui.js` library. They provide
scrollable, selectable lists of generic *items*. Those items are not restricted to simple string types - `Lists` can be used
to display heterogenous collections of arbitrary objects. The goal is to avoid having to map application-specific data 
(e.g. `GoesrDataSet`) into UI-displayable data. This is achieved by supporting a column-oriented display configuration that
uses function parameters to specify what/how to display for each column.

The four groups of `List` related functions are:

1. construction (`List (id, maxRows, selectAction, clickAction, contextMenuAction, dblClickAction)`)
2. display configuration (`setListItemDisplayColumns (listElement, listAttrs, colSpecs)`)
3. data management (`setListItems (listElement, items)`, `updateListItem (listElement, item)`,
   `appendListItem (listElement, item)`, `removeListItem (listElement, item)`, `clearList (itemElement)`)
4. selection and selection callbacks (`setSelectedListItem(listElement, item)`, single- and double-click event handler
   functions)

The general pattern of using `List` instances therefore is:

```javascript
import * as ui from "../odin_server/ui.js";
...
var datasSetList = undefined;
var selectedDataSet = undefined;

createWindow();
initDataSetList();
...
function createWindow () {
    return ui.Window(...)(
        ...
        (dataSetList = ui.List("goesr.dataSets", 6, selectGoesrDataSet)), // ① List construction
        ...
    );
}

function initDataSetList () {  // ② display configuration
    if (dataSetList) {
        ui.setListItemDisplayColumns(dataSetList, ["fit", "header"], [
            { name: "sat", tip: "name of satellite", width: "3rem", attrs: [], map: e => e.sat.name },
            { name: "good", tip: "number of good pixels", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.nGood },
            ...
            { name: "date", tip: "last report", width: "8rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMString(e.date) }
        ]);
    }
}

... ui.setListItems( dataSetList, dataSets); ... // ③ data management
... ui.updateListItem( dataSetList, selectedDataSet); ...

... ui.setSelectedListItem( dataSetList, matchingItem); // ④ selection and selection callbacks

function selectGoesrDataSet (event) { // user selected item with single click
    let item = event.detail.curSelection;
    if (item) {
        selectedDataSet = item;
        ...
    }
}
```

The `List` widget has a rich API that supports many more functions, reflecting that ODIN data is generally dynamic in nature and we
therefore need to support efficient update.

`List` item columns do not have to map 1:1 into item properties. Columns can contain computed data and even widgets (e.g.
`CheckBoxes`).


#### `TreeList`

`TreeList` is a close cousin of `List` that is used to display hierarchical data that can be interactively expanded/collapsed. In
fact, `TreeList` is implemented as a `List` that wraps application-specific *items* into generic *nodes* that retain the
column-oriented item display configuration for leaf- nodes. The mapping from application specific item collections into
generic nodes is based on the `ExpandableTreeNode` class in the `ui_data.js` module of [`odin_server`](../odin_server/odin_server.md).
Since `ExpandableTreeNode` is a display (UI) specific type this means data initialization of `TreeLists` always involves
constructing a tree from application specific item collections like so:

```javascript
import * as ui from "../odin_server/ui.js";
import * as uiData from "../odin_server/ui_data.js";
...
   let tree = uiData.ExpandableTreeNode.from( items, e=>e.key);
   ui.setTree( dirView, tree);
...
```

The second parameter of `ExpandableTreeNode.from (items, pathExtractor, ..)` is a function (usually provided by a closure) that
defines how to compute the hierarchical pathname of an item that is the basis for constructing the tree.

Apart from the tree construction and the addition of item-wrapping nodes the `TreeList` API has the same categories as `List`:
construction, display configuration, data management and selection management. The typical pattern of `TreeList` usage therefore
is:

```javascript
import * as ui from "../odin_server/ui.js";
import * as uiData from "../odin_server/ui_data.js";
...
function createWindow(..){
   ... (dirList = ui.TreeList("share.dir.list", 15, "32rem", selectShareEntry)), ...
}

function initDirList() {
    if (dirList) {
        ui.setListItemDisplayColumns(dirList, ["fit", "header"], [
            { name: "show", tip: "render selected item", width: "2.5rem", attrs:[], map: e=>itemRenderCheckBox(e) },
            ...
            { name: "type", tip: "item type", width: "6rem", attrs: ["small"], map: e=> itemType(e) }
        ]);
    }
}
function itemRenderCheckBox (e) {
    return e && e.value ? ui.createCheckBox( isItemShowing(e), toggleShowItem) : "";
}

  ... let tree = ExpandableTreeNode.from( items, e=>e.key);
      ui.setTree( dirView, tree); ...

```


#### Icon

#### Panel

#### Button

#### CheckBox

#### Radio

#### Choice

#### Slider



#### Label and VarText

#### Field

#### TextArea

#### PopupMenu

#### RowContainer and ColumnContainer
