import { rangeFillStyle } from "./rangeFill";

/** Slider with its label above and the current value at its end. */
export function RangeField({
  id,
  label,
  value,
  min,
  max,
  step = 1,
  disabled = false,
  describedBy,
  display,
  onChange,
}: {
  id: string;
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  disabled?: boolean;
  /** Id of a help text that describes the slider. */
  describedBy?: string | undefined;
  display: string;
  onChange: (value: number) => void;
}) {
  return (
    <div className="range-field">
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        style={rangeFillStyle(value, min, max)}
        aria-valuetext={display}
        aria-describedby={describedBy}
        onChange={(event) => {
          onChange(Number(event.currentTarget.value));
        }}
      />
      <output htmlFor={id}>{display}</output>
    </div>
  );
}
