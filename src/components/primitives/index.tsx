import * as RadixDialog from "@radix-ui/react-dialog";
import * as RadixMenu from "@radix-ui/react-dropdown-menu";
import * as RadixTabs from "@radix-ui/react-tabs";
import * as RadixTooltip from "@radix-ui/react-tooltip";
import { Children, Fragment, forwardRef } from "react";
import type { ButtonHTMLAttributes, CSSProperties, HTMLAttributes, InputHTMLAttributes, LiHTMLAttributes, MouseEvent, ReactNode } from "react";
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

/** The one search box every page toolbar uses: same height, width, colour and place. */
export function SearchField({ value, onChange, placeholder, label, className, children, autoFocus }: {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  /** read aloud; the placeholder when left out */
  label?: string;
  className?: string;
  /** extra buttons inside the box, after the input (saved searches) */
  children?: ReactNode;
  autoFocus?: boolean;
}) {
  return (
    <label className={cx("search ui-search", className)}>
      <Icon name="search" />
      <input value={value} placeholder={placeholder} aria-label={label ?? placeholder} autoFocus={autoFocus} spellCheck={false} onChange={(event) => onChange(event.target.value)} />
      {value && (
        <button type="button" className="ui-search-clear" aria-label={tr("Clear search")} onClick={() => onChange("")}>
          <Icon name="close" />
        </button>
      )}
      {children}
    </label>
  );
}

export type FilterTab<T extends string> = { value: T; label: ReactNode; count?: number | null; dot?: boolean; tone?: "danger" };

/** The one row of category buttons every page uses (General · Gameplay, All · Whitelist, Logs · Summary). */
export function FilterTabs<T extends string>({ value, options, onChange, label, className }: {
  value: T;
  options: FilterTab<T>[];
  onChange: (value: T) => void;
  label: string;
  className?: string;
}) {
  return (
    <div className={cx("ui-tabs", className)} role="tablist" aria-label={label}>
      {options.map((one) => (
        <button
          key={one.value}
          type="button"
          role="tab"
          aria-selected={one.value === value}
          className={cx("ui-tab", one.value === value && "on", one.tone)}
          onClick={() => onChange(one.value)}
        >
          {one.label}
          {one.count !== undefined && one.count !== null && <b>{one.count}</b>}
          {one.dot && <i className="ui-tab-dot" aria-hidden />}
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

export function DropdownMenu({ trigger, items, align = "end" }: {
  trigger: ReactNode;
  items: { label: ReactNode; icon?: string; danger?: boolean; disabled?: boolean; checked?: boolean; heading?: ReactNode; hint?: ReactNode; onSelect: () => void }[];
  align?: "start" | "end";
}) {
  return (
    <RadixMenu.Root>
      <RadixMenu.Trigger asChild>{trigger}</RadixMenu.Trigger>
      <RadixMenu.Portal>
        <RadixMenu.Content className="menu" align={align} sideOffset={6}>
          {items.map((one, index) => (
            <Fragment key={index}>
              {one.heading && (
                <>
                  {index > 0 && <RadixMenu.Separator className="menu-sep" />}
                  <RadixMenu.Label className="menu-label">{one.heading}</RadixMenu.Label>
                </>
              )}
              <RadixMenu.Item className={cx("menu-item", one.danger && "danger", one.checked && "checked")} disabled={one.disabled} onSelect={one.onSelect}>
                {one.icon && <Icon name={one.icon} />}
                <span className="menu-item-text">{one.label}</span>
                {one.hint && <small className="menu-item-hint">{one.hint}</small>}
                {one.checked && <Icon name="check" />}
              </RadixMenu.Item>
            </Fragment>
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

export type ListProps = Omit<HTMLAttributes<HTMLUListElement>, "children"> & {
  children?: ReactNode;
  /** card: filled box; box: bordered box; plain: no box (already inside a Card) */
  variant?: "card" | "box" | "plain";
  /** auto follows the app density; compact forces 40px rows */
  density?: "auto" | "compact";
  label?: string;
  /** shown instead of the list when it has no rows */
  empty?: ReactNode;
  /** grid columns for the meta slot, e.g. "110px 140px", for table-like lists with a ListHeader */
  columns?: string;
  /** width of the actions column when columns is set (default 40px) */
  actionsWidth?: string;
};

export function List({ children, variant = "card", density = "auto", label, empty, columns, actionsWidth, className, style, ...rest }: ListProps) {
  if (empty !== undefined && !Children.toArray(children).length) return <>{empty}</>;
  const shown = columns ? ({ ...style, "--list-cols": columns, ...(actionsWidth ? { "--list-act": actionsWidth } : {}) } as CSSProperties) : style;
  return (
    <ul
      className={cx("list", `list-${variant}`, density === "compact" && "list-compact", columns && "list-cols", className)}
      aria-label={label}
      style={shown}
      {...rest}
    >
      {children}
    </ul>
  );
}

export function ListGroup({ title, count, aside, children, className }: {
  title: ReactNode;
  count?: ReactNode;
  aside?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <li className={cx("list-group", className)}>
      <div className="list-group-head">
        <h3>{title}</h3>
        {count !== undefined && count !== null && <b>{count}</b>}
        {aside && <span className="list-group-aside">{aside}</span>}
      </div>
      <ul className="list-group-rows">{children}</ul>
    </li>
  );
}

export function ListHeader({ leading, title, meta, actions, className }: {
  leading?: ReactNode;
  title: ReactNode;
  meta?: ReactNode | ReactNode[];
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <li className={cx("list-header", className)}>
      {leading !== undefined && <span className="list-lead">{leading}</span>}
      <span className="list-main">{title}</span>
      {meta !== undefined && <ListMeta meta={meta} />}
      {actions !== undefined && <span className="list-actions">{actions}</span>}
    </li>
  );
}

function ListMeta({ meta }: { meta: ReactNode | ReactNode[] }) {
  return (
    <div className="list-meta">
      {Array.isArray(meta) ? meta.map((one, index) => <span key={index}>{one}</span>) : meta}
    </div>
  );
}

export type ListRowProps = Omit<LiHTMLAttributes<HTMLLIElement>, "title" | "onClick"> & {
  leading?: ReactNode;
  title: ReactNode;
  subtitle?: ReactNode;
  meta?: ReactNode | ReactNode[];
  actions?: ReactNode;
  /** makes the row a real button (the whole row is the hit area; actions stay separate) */
  onClick?: (event: MouseEvent<HTMLElement>) => void;
  /** makes the row a real link instead */
  href?: string;
  selected?: boolean;
  disabled?: boolean;
  /** extra attributes for the main button/link (aria-*, data-tip, onDoubleClick…) */
  mainProps?: HTMLAttributes<HTMLElement> & { [key: `data-${string}`]: string | undefined };
  /** content shown under the row, full width (expanded diff, inline editor) */
  detail?: ReactNode;
};

export const ListRow = forwardRef<HTMLLIElement, ListRowProps>(function ListRow(
  { leading, title, subtitle, meta, actions, onClick, href, selected, disabled, mainProps, detail, className, ...rest },
  ref,
) {
  const body = (
    <>
      <span className="list-title">{title}</span>
      {subtitle !== undefined && subtitle !== null && subtitle !== false && <span className="list-sub">{subtitle}</span>}
    </>
  );
  const main = href ? (
    <a className="list-main" href={disabled ? undefined : href} aria-disabled={disabled || undefined} onClick={onClick} {...mainProps}>
      {body}
    </a>
  ) : onClick ? (
    <button type="button" className="list-main" disabled={disabled} onClick={onClick} {...mainProps}>
      {body}
    </button>
  ) : (
    <div className="list-main" {...mainProps}>
      {body}
    </div>
  );
  return (
    <li
      ref={ref}
      className={cx("list-row", (onClick || href) && "interactive", selected && "selected", disabled && "disabled", detail ? "has-detail" : false, className)}
      {...rest}
    >
      {leading !== undefined && leading !== null && <span className="list-lead">{leading}</span>}
      {main}
      {meta !== undefined && meta !== null && <ListMeta meta={meta} />}
      {actions !== undefined && actions !== null && actions !== false && <div className="list-actions">{actions}</div>}
      {detail && <div className="list-detail">{detail}</div>}
    </li>
  );
});
