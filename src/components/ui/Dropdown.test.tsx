import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, test, vi } from "vitest";
import { Dropdown } from "./Dropdown";

const options = [
  { value: "8", label: "Java 8" },
  { value: "21", label: "Java 21", hint: "recommended" },
  { value: "17", label: "Java 17" },
  { value: "25", label: "Java 25" },
];

test("shows the picked label and opens the list in body", async () => {
  const user = userEvent.setup();
  const onChange = vi.fn();
  render(<Dropdown value="21" options={options} onChange={onChange} />);

  const trigger = screen.getByRole("button", { expanded: false });
  expect(trigger).toHaveTextContent("Java 21");
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();

  await user.click(trigger);
  expect(screen.getByRole("listbox")).toBeInTheDocument();
  const items = screen.getAllByRole("option");
  expect(items.map((one) => one.getAttribute("aria-selected"))).toEqual(["false", "true", "false", "false"]);

  await user.click(items[0]);
  expect(onChange).toHaveBeenCalledWith("8");
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
});

test("shows a placeholder without a value", () => {
  render(<Dropdown value="" options={options} onChange={() => {}} placeholder="Pick one" />);
  expect(screen.getByRole("button", { expanded: false })).toHaveTextContent("Pick one");
});

test("two or three short choices show as pills", async () => {
  const user = userEvent.setup();
  const onChange = vi.fn();
  render(<Dropdown value="21" options={options.slice(0, 2)} onChange={onChange} />);

  const pills = screen.getAllByRole("radio");
  expect(pills.map((one) => one.getAttribute("aria-checked"))).toEqual(["false", "true"]);
  await user.click(pills[0]);
  expect(onChange).toHaveBeenCalledWith("8");
});
