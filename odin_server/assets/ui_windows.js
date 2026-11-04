import * as util from "./ui_util.js";
import * as ui from "./ui.js";

// if `trackAction` is provided the window has a "track viewpoint" checkbox. If checked, mouse positions over the image are
// shown with a vertical and a horizontal line and reported as `trackAction({x, y, width, height, isFrozen})`, with x,y
// relative to the image center in [-1,1] (x positive to the right, y positive upwards) and width,height being the image
// size in pixels. Clicking on the image freezes/unfreezes the current position. `trackAction(undefined)` is called when
// tracking is stopped or the mouse leaves the image (unless the position is frozen)
export function ImageWindow (title, id, closeAction, imgUri, caption,
                             viewerWidth=600, viewerHeight=500, x=undefined, y=undefined,
                             minScale=0, maxScale=2.0, scaleStep=0.05, trackAction=undefined) {
    let isFullScale = false;
    let fullScaleActivated = false;
    let scale = 1;
    let initScale = 1;
    let imgWidth = 1; // needs to be finite so that we have a valid canvas size for initial (invisible) layout
    let imgHeight = 1;

    let cc = ui.createElement("DIV", "ui_canvas_wrapper");
    cc.style.width = "100%";
    cc.style.height = "100%";
    cc.style.overflow = "auto";

    let canvas = ui.createElement("CANVAS", "ui_canvas");
    cc.appendChild( canvas);

    //--- viewpoint tracking (optional)
    let isTracking = false;
    let isFrozen = false;
    let trackPos = undefined; // { x, y } relative to image center in [-1,1]
    let vLine = undefined;
    let hLine = undefined;
    let cbTrack = undefined;

    if (trackAction) {
        cc.style.position = "relative"; // so that we can position the track lines within the (scrolled) canvas wrapper
        vLine = createTrackLine();
        hLine = createTrackLine();
        cc.appendChild( vLine);
        cc.appendChild( hLine);
        cbTrack = ui.CheckBox("track viewpoint", toggleTracking);

        canvas.addEventListener("mousemove", trackMouse);
        canvas.addEventListener("mouseleave", leaveImage);
        canvas.addEventListener("click", toggleFrozen);
    }

    let img = new Image();
    img.src = imgUri; // kicks off async loading
    console.log("loading ", imgUri, "...");

    let label = ui.Label( caption, id+".caption");

    let scaleSlider = ui.Slider("scale", null, setImgScale, "18rem");
    let cbFullSize = ui.CheckBox("fit", toggleFullScale);

    let imageViewer = ui.Window( title, id, "./asset/odin_server/img.svg")(
        cc,
        ui.RowContainer("align_left",null,null,false,"100%")(
            label,
            ui.HorizontalSpacer(1),
            scaleSlider,
            cbFullSize,
            ui.Button("1x", nativeScale),
            ui.Button("reset", resetSize),
            ui.Button("\u2b07", downloadImage),
            ...(cbTrack ? [ui.HorizontalSpacer(1), cbTrack] : [])
        )
    );

    imageViewer.style.resize = "both";
    imageViewer.style.overflow = "hidden";
    imageViewer.style.width = viewerWidth + "px";
    imageViewer.style.height = viewerHeight + "px";

    let wc = ui.getWindowContent(imageViewer);
    wc.style.boxSizing = "border-box";
    wc.style.width = "100%";
    wc.style.height = "calc( 100% - var(--titlebar-height) - 2*var(--titlebar-padding))";

    ui.setSliderRange( scaleSlider, minScale, maxScale, scaleStep, util.f_1);
    ui.setSliderValue( scaleSlider, scale);

    if (closeAction) imageViewer.closeAction = closeAction;

    img.addEventListener('load', () => {
        console.log(imgUri, " loaded.");
        // note the window has already been created at this point but is not yet visible
        let iw = img.naturalWidth;
        let ih = img.naturalHeight;

        let wScale = viewerWidth / iw;
        let hScale = viewerHeight / ih;
        scale = Math.floor( Math.min( wScale, hScale) / scaleStep) * scaleStep;

        ui.setSliderValue( scaleSlider, scale);
        initScale = scale;

        imgWidth = iw;
        imgHeight = ih;

        let w = iw * scale;
        let h = ih * scale;

        canvas.width = w;
        canvas.height = h;
        const ctx = canvas.getContext("2d");
        ctx.drawImage( img, 0, 0, w, h);

        ui.addWindow(imageViewer);
        ui.showWindow(imageViewer);
        ui.placeWindow(imageViewer, x, y);

        resizeObserver.observe(cc);
    });

    // replace the image (and optionally title and caption) without changing window size, scale and scroll offsets (unless
    // we are in 'fit' mode, in which case we re-scale to the new image size). The new image is only shown once it is loaded
    imageViewer.setImage = (newUri, newTitle=undefined, newCaption=undefined) => {
        let newImg = new Image();
        newImg.addEventListener('load', () => {
            let scrollLeft = cc.scrollLeft;
            let scrollTop = cc.scrollTop;

            img = newImg;
            imgUri = newUri;
            imgWidth = newImg.naturalWidth;
            imgHeight = newImg.naturalHeight;

            if (isFullScale) {
                fullScaleActivated = true; // don't reset fullScale if this triggers a slider change event (which is synchronous)
                scaleFromSize( cc.offsetWidth, cc.offsetHeight);
                fullScaleActivated = false;
            }
            redrawImage();

            cc.scrollLeft = scrollLeft;
            cc.scrollTop = scrollTop;

            reportTrackPos(); // the image (and hence the camera state) changed
        });
        newImg.src = newUri; // kicks off async loading

        if (newTitle !== undefined) ui.setWindowTitle( imageViewer, newTitle);
        if (newCaption !== undefined) label.innerText = newCaption;
    };

    function toggleFullScale (event) {
        isFullScale = ui.isCheckBoxSelected(event.target);
        if (isFullScale) {
            fullScaleActivated = true;
            scaleToFullSize();
        }
    }

    function setImgScale (event) {
        if (imgWidth > 1){
            if (isFullScale) {
                if (fullScaleActivated) {
                    fullScaleActivated = false;
                } else {
                    isFullScale = false;
                    ui.setCheckBox( cbFullSize, isFullScale);
                }
            }

            scale = ui.getSliderValue(event.target);
            redrawImage();
        }
    }

    function redrawImage () {
        let w = scale * imgWidth;
        let h = scale * imgHeight;

        canvas.width = w;
        canvas.height = h;

        const ctx = canvas.getContext("2d");
        if (scale < 1.0) { ctx.clearRect(0,0,w,h); }
        ctx.drawImage( img, 0, 0, w, h);

        updateTrackLines();
    }

    //--- viewpoint tracking

    function createTrackLine() {
        let e = ui.createElement("DIV", "ui_track_line");
        e.style.position = "absolute";
        e.style.pointerEvents = "none"; // don't interfere with canvas mouse events
        e.style.background = "rgba(255,255,0,0.8)";
        e.style.display = "none";
        return e;
    }

    function toggleTracking (event) {
        setTracking( ui.isCheckBoxSelected(event.target));
    }

    function setTracking (enable) {
        isTracking = enable;
        canvas.style.cursor = isTracking ? "crosshair" : "default";
        if (isTracking) {
            document.addEventListener("keydown", trackKeyHandler);
        } else {
            document.removeEventListener("keydown", trackKeyHandler);
            isFrozen = false;
            clearTrackPos();
        }
    }

    // Esc exits tracking mode. This is a document listener since the image viewer does not get the keyboard focus
    function trackKeyHandler (event) {
        if (!imageViewer.isConnected) { // window was closed while tracking
            document.removeEventListener("keydown", trackKeyHandler);
        } else if (event.key === "Escape" && isTracking) {
            ui.setCheckBox( cbTrack, false);
            setTracking( false);
            event.preventDefault();
        }
    }

    function trackMouse (event) {
        if (isTracking && !isFrozen && canvas.width > 1 && canvas.height > 1) {
            trackPos = {
                x: (event.offsetX - canvas.width/2) / (canvas.width/2),
                y: (canvas.height/2 - event.offsetY) / (canvas.height/2)
            };
            updateTrackLines();
            reportTrackPos();
        }
    }

    function leaveImage (event) {
        if (isTracking && !isFrozen) clearTrackPos();
    }

    function toggleFrozen (event) {
        if (isTracking) {
            isFrozen = !isFrozen;
            if (!isFrozen) trackMouse(event); else reportTrackPos();
        }
    }

    function clearTrackPos() {
        trackPos = undefined;
        updateTrackLines();
        trackAction(undefined);
    }

    function reportTrackPos() {
        if (trackPos) trackAction( { x: trackPos.x, y: trackPos.y, width: imgWidth, height: imgHeight, isFrozen });
    }

    // position the track lines in canvas coordinates (they are children of the scrolled wrapper)
    function updateTrackLines() {
        if (!vLine) return;
        if (trackPos) {
            let px = canvas.offsetLeft + (trackPos.x + 1) * canvas.width/2;
            let py = canvas.offsetTop + (1 - trackPos.y) * canvas.height/2;
            let clr = isFrozen ? "rgba(255,64,0,0.9)" : "rgba(255,255,0,0.8)";

            Object.assign( vLine.style, { display: "block", left: `${Math.round(px)}px`, top: `${canvas.offsetTop}px`,
                                         width: "1px", height: `${canvas.height}px`, background: clr });
            Object.assign( hLine.style, { display: "block", left: `${canvas.offsetLeft}px`, top: `${Math.round(py)}px`,
                                         width: `${canvas.width}px`, height: "1px", background: clr });
        } else {
            vLine.style.display = "none";
            hLine.style.display = "none";
        }
    }

    function resetSize (event) {
        imageViewer.style.width = viewerWidth + "px";
        imageViewer.style.height = viewerHeight + "px";
        ui.setSliderValue( scaleSlider, initScale);
    }

    function nativeScale (event) {
        ui.setSliderValue(scaleSlider, 1.0);
    }

    function scaleToFullSize() {
        let ccWidth = cc.offsetWidth;
        let ccHeight = cc.offsetHeight;
        scaleFromSize( ccWidth, ccHeight);
    }

    let resizeObserver = new ResizeObserver((entries) => {
        if (isFullScale) {
            setTimeout( () => {
                fullScaleActivated = true; // don't reset fullScale
                scaleFromSize( cc.offsetWidth, cc.offsetHeight);
            }, 100);
        }
    });

    function scaleFromSize (ccWidth, ccHeight) {
        let wScale = ccWidth / imgWidth;
        let hScale = ccHeight / imgHeight;
        scale = Math.floor( Math.min( wScale, hScale) / scaleStep) * scaleStep;
        ui.setSliderValue( scaleSlider, scale);
    }

    function downloadImage(event) {
        // TODO - this doesn't work for external images if server has wrong CORS policy (use proxy in this case)
        const link = document.createElement('a');
        link.href = imgUri;
        link.download = util.filename(imgUri);
        imageViewer.appendChild(link);
        link.click();
        imageViewer.removeChild(link);
    }

    return imageViewer;
}
