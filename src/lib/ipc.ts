import { commands as raw } from "./bindings";
import type { Sent } from "./sent";
import { tracked } from "./jobs";

type Raw = typeof raw;
type SentCommands = {
  [K in keyof Raw]: Raw[K] extends (...args: infer A) => Promise<infer R>
    ? (...args: A) => Promise<Sent<R>>
    : never;
};

const wrapped = new Proxy(raw, {
  get(target, key, receiver) {
    const value = Reflect.get(target, key, receiver);
    if (typeof value !== "function" || typeof key !== "string") return value;
    return (...args: unknown[]) => tracked(key, args, () => (value as (...a: unknown[]) => Promise<unknown>)(...args));
  },
});

export const commands = wrapped as unknown as SentCommands;
export type { Sent };
