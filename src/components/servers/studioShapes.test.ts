import { describe, expect, it } from "vitest";
import { armorModel, autoUv, cubeModel, jsonModel, plantModel, textureName, spriteModel } from "./studioShapes";

describe("pack 3D preview shapes", () => {
  it("draws a block with its own top", () => {
    const read = JSON.parse(cubeModel("data:image/png;base64,side", "data:image/png;base64,top"));
    expect(read.textures).toHaveLength(2);
    expect(read.elements[0].faces.up.texture).toBe(1);
    expect(read.elements[0].faces.north.texture).toBe(0);
    expect(read.elements[0].faces.down.texture).toBe(1);
  });

  it("crosses a plant", () => {
    const read = JSON.parse(plantModel(null));
    expect(read.elements.map((one: { rotation: number[] }) => one.rotation[1])).toEqual([45, -45]);
    expect(read.textures).toHaveLength(0);
  });

  it("follows a model's texture references to studio pictures", () => {
    const source = JSON.stringify({
      textures: { "0": "monolab:studio/ruby", particle: "#0", stone: "minecraft:block/stone" },
      elements: [{ from: [0, 0, 0], to: [16, 8, 16], rotation: { angle: 22.5, axis: "y", origin: [8, 8, 8] }, faces: { up: { texture: "#particle" }, north: { texture: "#stone", uv: [0, 8, 16, 16] } } }],
    });
    const read = JSON.parse(jsonModel(source, { ruby: "data:image/png;base64,r" }));
    expect(read.textures).toHaveLength(1);
    expect(read.elements[0].faces.up).toEqual({ uv: [0, 0, 16, 16], texture: 0 });
    expect(read.elements[0].faces.north.texture).toBeUndefined();
    expect(read.elements[0].rotation).toEqual([0, 22.5, 0]);
    expect(textureName("minecraft:block/stone")).toBeNull();
    expect(textureName("monolab:studio/ruby")).toBe("ruby");
    expect(autoUv("east", [2, 0, 4], [14, 8, 12])).toEqual([4, 8, 12, 16]);
  });

  it("puts armor on a stand-in player", () => {
    const read = JSON.parse(armorModel("legs", "data:image/png;base64,a"));
    const armor = read.elements.filter((one: { uuid: string }) => one.uuid.startsWith("armor-"));
    expect(armor.map((one: { uuid: string }) => one.uuid)).toEqual(["armor-body", "armor-rightLeg", "armor-leftLeg"]);
    expect(armor[0].from).toEqual([-4.5, 11.5, -2.5]);
    expect(read.resolution).toEqual({ width: 64, height: 32 });
  });
});

describe("spriteModel", () => {
  it("extrudes each row's solid runs into one box", () => {
    // A 4×2 picture: row 0 is solid at x 0-1 and 3, row 1 is empty.
    const solid = (x: number, y: number) => y === 0 && x !== 2;
    const read = JSON.parse(spriteModel("data:image/png;base64,", 4, 2, solid)) as { elements: { from: number[]; to: number[]; faces: Record<string, { uv: number[] }> }[] };
    expect(read.elements).toHaveLength(2);
    expect(read.elements[0].faces.south.uv).toEqual([0, 0, 2, 1]);
    expect(read.elements[0].faces.north.uv).toEqual([2, 0, 0, 1]);
    expect(read.elements[1].from[0]).toBeCloseTo(12);
  });

  it("is a grey card without a picture", () => {
    expect((JSON.parse(spriteModel(null)) as { elements: unknown[] }).elements).toHaveLength(1);
  });
});
