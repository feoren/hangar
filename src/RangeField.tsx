/** Slider with its label above and the current value at its end. */
export function RangeField({
  id,
  label,
  value,
  min,
  max,
  step = 1,
  disabled = false,
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
        aria-valuetext={display}
        onChange={(event) => {
          onChange(Number(event.currentTarget.value));
        }}
      />
      <output htmlFor={id}>{display}</output>
    </div>
  );
}
