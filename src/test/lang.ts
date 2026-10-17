import koFile from "../lang/ko.json";
import rustFile from "../lang/rust.ko.json";
import { flatten } from "../lib/i18n";

export const ko = flatten(koFile);
export const rustKo = flatten(rustFile);
