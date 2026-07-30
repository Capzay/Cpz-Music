import { describe, expect, it } from "vitest";
import { electActive, type DevicePresence } from "./realtime";

function device(deviceId: string, claimedAt: number): DevicePresence {
  return { deviceId, name: deviceId, platform: "Browser", claimedAt };
}

describe("electActive", () => {
  it("picks the most recent claim", () => {
    expect(electActive([device("a", 100), device("b", 300), device("c", 200)])).toBe("b");
  });

  it("breaks ties deterministically so every client agrees", () => {
    const devices = [device("z", 0), device("a", 0), device("m", 0)];
    expect(electActive(devices)).toBe("a");
    // Order of arrival must not change the outcome.
    expect(electActive([...devices].reverse())).toBe("a");
  });

  it("hands over automatically when the active device drops out", () => {
    const all = [device("phone", 500), device("laptop", 200)];
    expect(electActive(all)).toBe("phone");
    expect(electActive(all.filter((d) => d.deviceId !== "phone"))).toBe("laptop");
  });

  it("returns nothing when no device is present", () => {
    expect(electActive([])).toBeNull();
  });

  it("does not mutate the input", () => {
    const devices = [device("a", 1), device("b", 2)];
    electActive(devices);
    expect(devices.map((d) => d.deviceId)).toEqual(["a", "b"]);
  });
});
