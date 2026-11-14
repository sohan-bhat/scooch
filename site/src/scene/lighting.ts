import { ShaderChunk } from 'three';

const enter = 'getSpotLightInfo( spotLight, geometryPosition, directLight );';
const leave = 'RE_Direct( directLight, geometryPosition, geometryNormal, geometryViewDir, geometryClearcoatNormal, material, reflectedLight );';
const chunk = ShaderChunk.lights_fragment_begin;
const start = chunk.indexOf(enter);
const end = chunk.indexOf(leave, start);
if (start < 0 || end < 0 || chunk.indexOf('#pragma unroll_loop_end', start) < end) throw new Error('three spot light loop changed');
ShaderChunk.lights_fragment_begin = `${chunk.slice(0, start + enter.length)}\nif ( directLight.visible ) {${chunk.slice(start + enter.length, end + leave.length)}\n}${chunk.slice(end + leave.length)}`;
