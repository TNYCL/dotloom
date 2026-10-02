import type { Tool } from '../types.js'
import { ArcTool, CircleTool, DimensionTool, LineTool, PathTool, PolylineTool, RectTool, TextTool } from './draw.js'
import { ExtendTool, MoveTool, RotateTool, ScaleTool, SplitTool, TrimTool } from './modify.js'
import { PanTool, SelectTool } from './select.js'

export {
  ArcTool,
  CircleTool,
  DimensionTool,
  ExtendTool,
  LineTool,
  MoveTool,
  PanTool,
  PathTool,
  PolylineTool,
  RectTool,
  RotateTool,
  ScaleTool,
  SelectTool,
  SplitTool,
  TextTool,
  TrimTool,
}

/** Fresh instances of all built-in tools. */
export function builtinTools(): Tool[] {
  return [
    new SelectTool(),
    new PanTool(),
    new LineTool(),
    new PolylineTool(),
    new RectTool(),
    new CircleTool(),
    new ArcTool(),
    new PathTool(),
    new TextTool(),
    new DimensionTool(),
    new MoveTool(),
    new RotateTool(),
    new ScaleTool(),
    new SplitTool(),
    new TrimTool(),
    new ExtendTool(),
  ]
}
