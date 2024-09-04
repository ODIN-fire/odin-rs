/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
 * the National Aeronautics and Space Administration. All rights reserved.
 *
 * The “ODIN” software is licensed under the Apache License, Version 2.0 (the "License"); 
 * you may not use this file except in compliance with the License. You may obtain a copy 
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software distributed under
 * the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
 * either express or implied. See the License for the specific language governing permissions
 * and limitations under the License.
 */

import * as ui from "../odin_server/ui.js";

//ImageWindow (title, id, closeAction, icon, imgUri, caption, minScale, maxScale, initScale, w, h)
let w = ui.ImageWindow( 
    "Image Viewer",
    "img1",
    ()=>{console.log("closed image viewer")},
    "./asset/odin_server/settings.svg", 
    "./asset/odin_server/fire.webp",
    "test fire",
    0.5, 2.0, 0.1, 1.0
);

ui.addWindow(w);
ui.setWindowLocation(w, 100,100);
ui.showWindow(w);

