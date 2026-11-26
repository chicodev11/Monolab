import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { inputsOf, shapeFromLayers, WorldgenNodes } from "./StudioWorldgenNodes";
import type { StudioData } from "./StudioPreview";

const shape: StudioData = { type: "add", inputs: [{ type: "noise", scale: 100 }, { type: "terrace", steps: 3, input: { type: "noise" } }] };

describe("WorldgenNodes", () => {
  it("draws every node with a line to what feeds it", () => {
    render(<WorldgenNodes shape={shape} onChange={() => {}} noises={["perlin"]} noiseLabel={(one) => one} />);
    expect(document.querySelectorAll(".studio-node")).toHaveLength(4);
    expect(document.querySelectorAll(".studio-nodes-edges path")).toHaveLength(3);
    expect(document.querySelector(".studio-node.on")?.textContent).toContain("Add together");
  });

  it("removes a picked input from the tree", () => {
    const onChange = vi.fn();
    render(<WorldgenNodes shape={shape} onChange={onChange} noises={["perlin"]} noiseLabel={(one) => one} />);
    // Boxes are laid out from the bottom up: the first is the plain noise.
    fireEvent.click(document.querySelectorAll(".studio-node")[0]);
    fireEvent.click(document.querySelector("[aria-label='Remove']")!);
    const next = onChange.mock.calls[0][0] as StudioData;
    expect(inputsOf(next)).toHaveLength(1);
    expect(inputsOf(next)[0].type).toBe("terrace");
  });

  it("turns layers into the same sum", () => {
    const made = shapeFromLayers([{ noise: "ridged", scale: 80, amount: 0.5 }]);
    expect(made).toEqual({ type: "add", inputs: [{ type: "noise", noise: "ridged", scale: 80, octaves: 4, amount: 0.5 }] });
  });
});
