import * as RadixDialog from "@radix-ui/react-dialog";
import * as RadixMenu from "@radix-ui/react-dropdown-menu";
import * as RadixTabs from "@radix-ui/react-tabs";
import * as RadixTooltip from "@radix-ui/react-tooltip";
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from "react";
import { Dropdown as ExistingDropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { tr } from "../../lib/i18n";
import "./primitives.css";

function cx(...parts: (string | false | null | undefined)[]) {
  return parts.filter(Boolean).join(" ");
}

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "danger" | "ghost";
  size?: "sm" | "md" | "lg";
  icon?: string;
};

export function Button({ variant = "secondary", size = "md", icon, className, children, type = "button", ...rest }: ButtonProps) {
  return (
    <button
      type={type}
      className={cx("button", variant, size === "sm" && "small", size === "lg" && "big", className)}
      {...rest}
    >
      {icon && <Icon name={icon} />}
      {children}
    </button>
  );
}

export function IconButton({ label, icon, className, type = "button", ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; icon: string }) {
  return (
    <button type={type} className={cx("icon-button", className)} aria-label={label} data-tip={label} {...rest}>
      <Icon name={icon} />
    </button>
  );
}

export function Input({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={cx("input", className)} {...rest} />;
}

export function Checkbox({ label, className, ...rest }: InputHTMLAttributes<HTMLInputElement> & { label: ReactNode }) {
  return (
    <label className={cx("checkbox", className)}>
      <input type="checkbox" {...rest} />
      <span>{label}</span>
    </label>
  );
}

export function Switch({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <label className={cx("switch", className)}>
      <input type="checkbox" {...rest} />
      <span />
    </label>
  );
}

export const Select = ExistingDropdown;

export function SegmentedControl<T extends string>({ value, options, onChange, className }: {
  value: T;
  options: { value: T; label: ReactNode }[];
  onChange: (value: T) => void;
  className?: string;
}) {
  return (
    <div className={cx("segmented", className)} role="radiogroup">
      {options.map((one) => (
        <button
          key={one.value}
          type="button"
          role="radio"
          aria-checked={one.value === value}
          className={cx(one.value === value && "on")}
          onClick={() => onChange(one.value)}
        >
          {one.label}
        </button>
      ))}
    </div>
  );
}

export function Card({ title, aside, children, className }: { title?: ReactNode; aside?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cx("card", className)}>
      {(title || aside) && (
        <header className="card-head">
          {title && <h3>{title}</h3>}
          {aside}
        </header>
      )}
      {children}
    </section>
  );
}

export function Badge({ tone = "line", children }: { tone?: "line" | "subtle" | "ok" | "warn" | "danger"; children: ReactNode }) {
  return <span className={cx("chip", tone !== "line" && tone)}>{children}</span>;
}

export function StatusDot({ status }: { status: string }) {
  return <span className={`status-dot status-${status}`} aria-label={status} />;
}

export function Banner({ tone = "info", children, action }: { tone?: "info" | "warn" | "danger"; children: ReactNode; action?: ReactNode }) {
  return (
    <div className={cx("banner", tone)} role={tone === "danger" ? "alert" : "status"}>
      <Icon name={tone === "info" ? "info" : "alert"} />
      <div className="banner-body">{children}</div>
      {action}
    </div>
  );
}

export function Skeleton({ height = 40, className }: { height?: number; className?: string }) {
  return <div className={cx("skeleton", "skeleton-line", className)} style={{ height }} aria-hidden />;
}

export function EmptyState({ icon = "info", title, hint, action }: { icon?: string; title: ReactNode; hint?: ReactNode; action?: ReactNode }) {
  return (
    <div className="tool-empty empty-block">
      <Icon name={icon} />
      <strong>{title}</strong>
      {hint && <p>{hint}</p>}
      {action}
    </div>
  );
}

export function ErrorState({ title, detail, retry }: { title: ReactNode; detail?: ReactNode; retry?: () => void }) {
  return (
    <div className="tool-empty empty-block error-block" role="alert">
      <Icon name="alert" />
      <strong>{title}</strong>
      {detail && <p>{detail}</p>}
      {retry && (
        <Button variant="secondary" size="sm" icon="refresh" onClick={retry}>
          {tr("Try again")}
        </Button>
      )}
    </div>
  );
}

export function Toast({ tone = "info", children }: { tone?: "info" | "good" | "error"; children: ReactNode }) {
  return (
    <div className={cx("toast", tone !== "info" && tone)}>
      <Icon name={tone === "error" ? "alert" : tone === "good" ? "check" : "info"} />
      <span>{children}</span>
    </div>
  );
}

export function Table({ columns, children, className }: { columns: ReactNode[]; children: ReactNode; className?: string }) {
  return (
    <table className={cx("table", className)}>
      <thead>
        <tr>
          {columns.map((one, index) => (
            <th key={index}>{one}</th>
          ))}
        </tr>
      </thead>
      <tbody>{children}</tbody>
    </table>
  );
}

export function Tabs({ value, onChange, items, className }: {
  value: string;
  onChange: (value: string) => void;
  items: { value: string; label: ReactNode; content: ReactNode }[];
  className?: string;
}) {
  return (
    <RadixTabs.Root value={value} onValueChange={onChange} className={cx("tabs", className)}>
      <RadixTabs.List className="tabs-list">
        {items.map((one) => (
          <RadixTabs.Trigger key={one.value} value={one.value} className="tabs-tab">
            {one.label}
          </RadixTabs.Trigger>
        ))}
      </RadixTabs.List>
      {items.map((one) => (
        <RadixTabs.Content key={one.value} value={one.value} className="tabs-panel">
          {one.content}
        </RadixTabs.Content>
      ))}
    </RadixTabs.Root>
  );
}

export function Dialog({ open, onOpenChange, title, children, actions }: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  children?: ReactNode;
  actions: ReactNode;
}) {
  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="ask-backdrop modal-backdrop" />
        <RadixDialog.Content className="ask-dialog dialog">
          <RadixDialog.Title asChild>
            <h2>{title}</h2>
          </RadixDialog.Title>
          {children && (
            <RadixDialog.Description asChild>
              <p className="modal-copy">{children}</p>
            </RadixDialog.Description>
          )}
          <div className="modal-actions">{actions}</div>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}

export function DropdownMenu({ trigger, items }: {
  trigger: ReactNode;
  items: { label: ReactNode; icon?: string; danger?: boolean; onSelect: () => void }[];
}) {
  return (
    <RadixMenu.Root>
      <RadixMenu.Trigger asChild>{trigger}</RadixMenu.Trigger>
      <RadixMenu.Portal>
        <RadixMenu.Content className="menu" align="end" sideOffset={6}>
          {items.map((one, index) => (
            <RadixMenu.Item key={index} className={cx("menu-item", one.danger && "danger")} onSelect={one.onSelect}>
              {one.icon && <Icon name={one.icon} />}
              {one.label}
            </RadixMenu.Item>
          ))}
        </RadixMenu.Content>
      </RadixMenu.Portal>
    </RadixMenu.Root>
  );
}

export function Tooltip({ text, children }: { text: string; children: ReactNode }) {
  return (
    <RadixTooltip.Provider delayDuration={350}>
      <RadixTooltip.Root>
        <RadixTooltip.Trigger asChild>{children}</RadixTooltip.Trigger>
        <RadixTooltip.Portal>
          <RadixTooltip.Content className="tip tip-radix" sideOffset={6}>
            {text}
          </RadixTooltip.Content>
        </RadixTooltip.Portal>
      </RadixTooltip.Root>
    </RadixTooltip.Provider>
  );
}
