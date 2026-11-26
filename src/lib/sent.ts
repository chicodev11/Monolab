import type { GameEvent_Serialize, LivePlayer_Serialize, LiveState_Serialize } from "./bindings";

type Split = GameEvent_Serialize | LivePlayer_Serialize | LiveState_Serialize;

export type Sent<T> = T extends Split
  ? T
  : T extends (infer U)[]
    ? Sent<U>[]
    : T extends object
      ? { [K in keyof T]-?: Sent<T[K]> }
      : T;
