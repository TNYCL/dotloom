# Examples

All examples use the same engine and the same plugin mechanism; their rules are
engine rules, not code in the example.

| Example | Shows |
|---|---|
| [Playground](/playground/) | the reference React editor with all three example plugins |
| [Shelf configurator](/examples/shelf-configurator/) | a host-built UI: 180 cm inner width, three compartments, the left one locked at 60 cm, the other two equal and at least 40 cm. 160 cm gives 60/50/50; 130 cm is rejected and 140 cm is offered as the nearest allowed width; unlock; undo/redo |
| [Room planner](/examples/floorplan/) | wall and door types, doors hosted on walls, connected walls, associative dimensions, layers, snapping. Shortening a wall slides its door or reports a conflict; deleting a wall deletes its doors; save/load keeps references |
| [Timeline](/examples/timeline/) | blocks with start, duration and end, ordering rules with minimum gaps, equal durations and a locked release time; the document's time axis maps hours to drawing units |
| [Vanilla](/examples/vanilla/) | no React: a plain DOM toolbar on `createEditor` |
| External plugin (`examples/external-plugin`) | a project outside the workspace using the packed npm packages and only public APIs |

The source of every example is in the repository under `examples/`; the shared
plugin definitions are in `examples/plugins`.
