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
import * as Util from "./windUtils.js";
import { CustomPrimitive } from "./customPrimitive.js";

export class ParticlesComputing {

    constructor(context, data, userInput, viewerParameters) {
        this.createWindTextures(context, data);
        this.createParticlesTextures(context, data, userInput, viewerParameters);
        this.createComputingPrimitives(data, userInput, viewerParameters);
    }

    createWindTextures(context, data) {        
        var windTextureOptions = {
            context: context,
            width: data.dimensions.lon,
            height: data.dimensions.lat * data.dimensions.lev,
            //pixelFormat: Cesium.PixelFormat.LUMINANCE, // not a valid combination for WebGL 2
            pixelFormat: Cesium.PixelFormat.RED, // single component or WebGL will complain about not enough data
            pixelDatatype: Cesium.PixelDatatype.FLOAT,
            flipY: false,
            sampler: new Cesium.Sampler({
                // the values of texture will not be interpolated
                minificationFilter: Cesium.TextureMinificationFilter.NEAREST,
                magnificationFilter: Cesium.TextureMagnificationFilter.NEAREST
            })
        };

        this.windTextures = {
            H: Util.createTexture(windTextureOptions, data.H.array),
            U: Util.createTexture(windTextureOptions, data.U.array),
            V: Util.createTexture(windTextureOptions, data.V.array),
            W: Util.createTexture(windTextureOptions, data.W.array),
        };
    }

    createParticlesTextures(context, data, userInput, viewerParameters) {
        var particlesTextureOptions = {
            context: context,
            width: userInput.particlesTextureSize,
            height: userInput.particlesTextureSize,
            pixelFormat: Cesium.PixelFormat.RGBA,
            pixelDatatype: Cesium.PixelDatatype.FLOAT,
            flipY: false,
            sampler: new Cesium.Sampler({
                // the values of texture will not be interpolated
                minificationFilter: Cesium.TextureMinificationFilter.NEAREST,
                magnificationFilter: Cesium.TextureMagnificationFilter.NEAREST
            })
        };

        var particlesArray = Util.randomizeParticles(data, userInput.maxParticles, viewerParameters);
        var zeroArray = new Float32Array(4 * userInput.maxParticles).fill(0);

        this.particlesTextures = {
            previousParticlesPosition: Util.createTexture(particlesTextureOptions, particlesArray),
            currentParticlesPosition: Util.createTexture(particlesTextureOptions, particlesArray),
            nextParticlesPosition: Util.createTexture(particlesTextureOptions, particlesArray),
            postProcessingPosition: Util.createTexture(particlesTextureOptions, particlesArray),

            particlesSpeed: Util.createTexture(particlesTextureOptions, zeroArray)
        };
    }

    destroyParticlesTextures() {
        Object.keys(this.particlesTextures).forEach((key) => {
            this.particlesTextures[key].destroy();
        });
    }

    createComputingPrimitives(data, userInput, viewerParameters) {
        let dimension = new Cesium.Cartesian3(data.dimensions.lon, data.dimensions.lat, data.dimensions.lev);
        let minimum = new Cesium.Cartesian3(data.lon.min, data.lat.min, data.lev.min);
        let maximum = new Cesium.Cartesian3(data.lon.max, data.lat.max, data.lev.max);
        let interval = new Cesium.Cartesian3(
            (maximum.x - minimum.x) / (dimension.x - 1),
            (maximum.y - minimum.y) / (dimension.y - 1),
            dimension.z > 1 ? (maximum.z - minimum.z) / (dimension.z - 1) : 1.0
        );
        //if (interval.z == 0) interval.z = 1; // this is used as a quotient in shaders - avoid divZero

        let uSpeedRange = new Cesium.Cartesian2(data.U.min, data.U.max);
        let vSpeedRange = new Cesium.Cartesian2(data.V.min, data.V.max);
        let wSpeedRange = new Cesium.Cartesian2(data.W.min, data.W.max);

        let that = this;

        this.primitives = {
            calculateSpeed: new CustomPrimitive({
                commandType: 'Compute',
                uniformMap: {
                    U: function() {
                        return that.windTextures.U;
                    },
                    V: function() {
                        return that.windTextures.V;
                    },
                    W: function() {
                        return that.windTextures.W;
                    },
                    currentParticlesPosition: function() {
                        return that.particlesTextures.currentParticlesPosition;
                    },
                    dimension: function() {
                        return dimension;
                    },
                    minimum: function() {
                        return minimum;
                    },
                    maximum: function() {
                        return maximum;
                    },
                    interval: function() {
                        return interval;
                    },
                    uSpeedRange: function() {
                        return uSpeedRange;
                    },
                    vSpeedRange: function() {
                        return vSpeedRange;
                    },
                    wSpeedRange: function() {
                        return wSpeedRange;
                    },
                    pixelSize: function() {
                        return viewerParameters.pixelSize;
                    },
                    speedFactor: function() {
                        return userInput.speedFactor;
                    }
                },
                fragmentShaderSource: new Cesium.ShaderSource({
                    sources: [Util.loadText('./asset/odin_wind/wind-particles/glsl/calculateSpeed.frag')]
                }),
                outputTexture: this.particlesTextures.particlesSpeed,
                preExecute: function() {
                    // swap textures before binding
                    var temp;
                    temp = that.particlesTextures.previousParticlesPosition;
                    that.particlesTextures.previousParticlesPosition = that.particlesTextures.currentParticlesPosition;
                    that.particlesTextures.currentParticlesPosition = that.particlesTextures.postProcessingPosition;
                    that.particlesTextures.postProcessingPosition = temp;

                    // keep the outputTexture up to date
                    that.primitives.calculateSpeed.commandToExecute.outputTexture = that.particlesTextures.particlesSpeed;
                }
            }),

            updatePosition: new CustomPrimitive({
                commandType: 'Compute',
                uniformMap: {
                    currentParticlesPosition: function() {
                        return that.particlesTextures.currentParticlesPosition;
                    },
                    particlesSpeed: function() {
                        return that.particlesTextures.particlesSpeed;
                    }
                },
                fragmentShaderSource: new Cesium.ShaderSource({
                    sources: [Util.loadText('./asset/odin_wind/wind-particles/glsl/updatePosition.frag')]
                }),
                outputTexture: this.particlesTextures.nextParticlesPosition,
                preExecute: function() {
                    // keep the outputTexture up to date
                    that.primitives.updatePosition.commandToExecute.outputTexture = that.particlesTextures.nextParticlesPosition;
                }
            }),

            postProcessingPosition: new CustomPrimitive({
                commandType: 'Compute',
                uniformMap: {
                    nextParticlesPosition: function() {
                        return that.particlesTextures.nextParticlesPosition;
                    },
                    particlesSpeed: function() {
                        return that.particlesTextures.particlesSpeed;
                    },
                    lonRange: function() {
                        //return viewerParameters.lonRange;
                        return new Cesium.Cartesian2(data.lon.min, data.lon.max);
                    },
                    latRange: function() {
                        //return viewerParameters.latRange;
                        return new Cesium.Cartesian2(data.lat.min, data.lat.max);
                    },
                    randomCoefficient: function() {
                        var randomCoefficient = Math.random();
                        return randomCoefficient;
                    },
                    dropRate: function() {
                        return userInput.dropRate;
                    },
                    dropRateBump: function() {
                        return userInput.dropRateBump;
                    }
                },
                fragmentShaderSource: new Cesium.ShaderSource({
                    sources: [Util.loadText('./asset/odin_wind/wind-particles/glsl/postProcessingPosition.frag')]
                }),
                outputTexture: this.particlesTextures.postProcessingPosition,
                preExecute: function() {
                    // keep the outputTexture up to date
                    that.primitives.postProcessingPosition.commandToExecute.outputTexture = that.particlesTextures.postProcessingPosition;
                }
            })
        }
    }

    forEachPrimitive(func) {
        func(this.primitives.calculateSpeed);
        func(this.primitives.updatePosition);
        func(this.primitives.postProcessingPosition);
    }

    refreshParticles(context,data,userInput,viewerParameters) {
        this.destroyParticlesTextures();
        this.createParticlesTextures(context, data, userInput, viewerParameters);
        
        this.updateUserInputUniforms(userInput)
    }

    updateUserInputUniforms (userInput) {
        let primitives = this.primitives;

        let map = primitives.calculateSpeed.uniformMap;
        const speedFactor = userInput.speedFactor;
        map.speedFactor = function() {
            return speedFactor;
        };

        map = primitives.postProcessingPosition.uniformMap;
        const dropRate = userInput.dropRate;
        map.dropRate = function() {
            return dropRate;
        };
        const dropRateBump = userInput.dropRateBump;
        map.dropRateBump = function() {
            return dropRateBump;
        }

    }
}