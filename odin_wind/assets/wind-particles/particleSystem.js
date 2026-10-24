/*
 * Copyright (c) 2023, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The RACE - Runtime for Airspace Concept Evaluation platform is licensed
 * under the Apache License, Version 2.0 (the "License"); you may not use
 * this file except in compliance with the License. You may obtain a copy
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
import { ParticlesComputing } from "./particlesComputing.js";
import { ParticlesRendering } from "./particlesRendering.js";

const MIN_PARTICLES_TEXTURE_SIZE = 8; // lower bound for the effective number of particles if any part of the grid is visible

export class ParticleSystem {
    constructor(context, data, userInput, viewerParameters) {
        this.context = context;
        this.data = data;
        this.userInput = {...userInput};

        // NOTE - the uniform functions of particlesComputing and particlesRendering close over this object, i.e. it has
        // to be updated in place (see applyViewerParameters). We also need our own lon/lat range objects
        this.viewerParameters = {
            lonRange: Cesium.Cartesian2.clone( viewerParameters.lonRange),
            latRange: Cesium.Cartesian2.clone( viewerParameters.latRange),
            pixelSize: viewerParameters.pixelSize,

            // the visible part of the grid in which we (re-)seed particles (computed by updateSeedRange)
            seedLonRange: new Cesium.Cartesian2( data.lon.min, data.lon.max),
            seedLatRange: new Cesium.Cartesian2( data.lat.min, data.lat.max)
        };
        this.visibleFraction = 1.0; // fraction of the view area that is covered by the grid
        this.updateSeedRange();

        let effectiveInput = this.effectiveUserInput();
        this.particlesComputing = new ParticlesComputing(
            this.context, this.data,
            effectiveInput, this.viewerParameters
        );
        this.particlesRendering = new ParticlesRendering(
            this.context, this.data,
            effectiveInput, this.viewerParameters,
            this.particlesComputing
        );
    }

    // compute the intersection of the view and the grid (in place) as the range in which we seed particles, and the
    // fraction of the view area that is covered by it. Longitude ranges are [min,max] intervals with min in [0,360) and
    // max potentially exceeding 360 if the interval crosses the prime meridian
    updateSeedRange() {
        let vp = this.viewerParameters;
        let data = this.data;

        let gridLon = { min: data.lon.min, max: data.lon.max };
        let gridLat = { min: data.lat.min, max: data.lat.max };
        let viewLon = { min: vp.lonRange.x, max: vp.lonRange.y };
        let viewLat = { min: vp.latRange.x, max: vp.latRange.y };

        let lon = intersectLonRanges( gridLon, viewLon);
        let lat = intersectRanges( gridLat, viewLat);

        if (lon && lat) {
            vp.seedLonRange.x = lon.min;
            vp.seedLonRange.y = lon.max;
            vp.seedLatRange.x = lat.min;
            vp.seedLatRange.y = lat.max;

            let viewArea = sphericalArea( viewLon, viewLat);
            this.visibleFraction = (viewArea > 0) ? Math.min( 1.0, sphericalArea( lon, lat) / viewArea) : 1.0;

        } else { // grid not in view - keep particles on the grid (they are not visible anyway)
            vp.seedLonRange.x = gridLon.min;
            vp.seedLonRange.y = gridLon.max;
            vp.seedLatRange.x = gridLat.min;
            vp.seedLatRange.y = gridLat.max;
            this.visibleFraction = 0.0;
        }
    }

    // the userInput with the number of particles we actually use. userInput.particlesTextureSize is for a view that is
    // completely covered by the grid - we scale it with the covered fraction of the view to get a height invariant
    // particle density on screen
    effectiveUserInput() {
        let ui = { ...this.userInput };
        let size = ui.particlesTextureSize;
        if (size > 0) {
            let minSize = Math.min( size, MIN_PARTICLES_TEXTURE_SIZE);
            size = Math.max( minSize, Math.round( size * Math.sqrt( this.visibleFraction)));
        }
        ui.particlesTextureSize = size;
        ui.maxParticles = size * size;
        return ui;
    }

    canvasResize(context) {
        this.particlesComputing.destroyParticlesTextures();
        Object.keys(this.particlesComputing.windTextures).forEach((key) => {
            this.particlesComputing.windTextures[key].destroy();
        });


        Object.keys(this.particlesRendering.framebuffers).forEach((key) => {
            this.particlesRendering.framebuffers[key].destroy();
        });

        this.context = context;
        let effectiveInput = this.effectiveUserInput();
        this.particlesComputing = new ParticlesComputing(
            this.context, this.data,
            effectiveInput, this.viewerParameters
        );
        this.particlesRendering = new ParticlesRendering(
            this.context, this.data,
            effectiveInput, this.viewerParameters,
            this.particlesComputing
        );
    }

    forEachPrimitive(func) {
        this.particlesComputing.forEachPrimitive(func);
        this.particlesRendering.forEachPrimitive(func);
    }

    release() {
        this.particlesComputing.destroyParticlesTextures();
        // TODO - frameBuffers and particlesRendering resources ?
    }

    clearFramebuffers() {
        var clearCommand = new Cesium.ClearCommand({
            color: new Cesium.Color(0.0, 0.0, 0.0, 0.0),
            depth: 1.0,
            framebuffer: undefined,
            pass: Cesium.Pass.OPAQUE
        });

        Object.keys(this.particlesRendering.framebuffers).forEach((key) => {
            clearCommand.framebuffer = this.particlesRendering.framebuffers[key];
            clearCommand.execute(this.context);
        });
    }

    refreshParticles() {
        let effectiveInput = this.effectiveUserInput(); // number of particles depends on visible grid fraction
        this.clearFramebuffers();
        this.particlesComputing.refreshParticles(this.context, this.data, effectiveInput, this.viewerParameters);
        this.particlesRendering.updateSegments(this.context, effectiveInput);
    }

    applyUserInput(userInput) {
        this.userInput = {...userInput};

        // preserve visibility - this might be called for particle systems that are not showing
        let shown = [];
        this.forEachPrimitive( p=> { shown.push(p.show); p.show = false; });

        this.particlesComputing.updateUserInputUniforms(userInput);
        this.particlesRendering.updateUserInputUniforms(userInput);
        this.refreshParticles();

        let i = 0;
        this.forEachPrimitive( p=> p.show = shown[i++]);
    }

    // update in place since uniform functions refer to our viewerParameters object
    applyViewerParameters(viewerParameters) {
        let vp = this.viewerParameters;
        Cesium.Cartesian2.clone( viewerParameters.lonRange, vp.lonRange);
        Cesium.Cartesian2.clone( viewerParameters.latRange, vp.latRange);
        vp.pixelSize = viewerParameters.pixelSize;

        this.updateSeedRange();
        this.refreshParticles();
    }
}
//--- range helpers (degrees)

function intersectRanges (a, b) {
    let min = Math.max( a.min, b.min);
    let max = Math.min( a.max, b.max);
    return (max > min) ? { min, max } : null;
}

// intersect two longitude intervals that are given as [min,max] with min in [0,360) and width <= 360 (i.e. max can exceed
// 360 for intervals crossing the prime meridian). The result is in the representation of `a` (we use it for the grid).
// If the intervals overlap in two pieces (both wide and wrapping) we return the larger one
function intersectLonRanges (a, b) {
    let wa = a.max - a.min;
    let wb = b.max - b.min;
    if (wb >= 360) return { min: a.min, max: a.max };
    if (wa >= 360) {
        let min = Cesium.Math.mod( b.min, 360);
        return { min, max: min + wb };
    }

    // shift b so that it starts within [a.min, a.min+360), then also check the interval one period earlier
    let d = Cesium.Math.mod( b.min - a.min, 360);
    let best = null;
    for (let start of [a.min + d, a.min + d - 360]) {
        let r = intersectRanges( a, { min: start, max: start + wb });
        if (r && (!best || (r.max - r.min) > (best.max - best.min))) best = r;
    }

    if (best && best.min >= 360) { best.min -= 360; best.max -= 360; } // keep min in [0,360)
    return best;
}

// relative area of a lon/lat rectangle on the sphere (we only need ratios)
function sphericalArea (lon, lat) {
    let dLon = Cesium.Math.toRadians( lon.max - lon.min);
    let dSin = Math.sin( Cesium.Math.toRadians( lat.max)) - Math.sin( Cesium.Math.toRadians( lat.min));
    return dLon * dSin;
}
