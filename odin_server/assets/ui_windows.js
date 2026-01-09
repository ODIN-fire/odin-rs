import * as util from "./ui_util.js";
import * as ui from "./ui.js";

export function ImageWindow (title, id, closeAction, imgUri, caption,
                             viewerWidth=600, viewerHeight=500, x=undefined, y=undefined,
                             minScale=0, maxScale=2.0, scaleStep=0.05) {
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
            ui.Button("\u2b07", downloadImage)
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
