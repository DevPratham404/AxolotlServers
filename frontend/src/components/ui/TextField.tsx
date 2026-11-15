import type { InputHTMLAttributes, ReactNode } from 'react'
import { useId } from 'react'

import { cn } from '../../lib/cn'

interface TextFieldProps extends InputHTMLAttributes<HTMLInputElement> {
  label: string
  hint?: ReactNode
  error?: string
  trailing?: ReactNode
}

export function TextField({
  label,
  hint,
  error,
  trailing,
  className,
  id,
  ...rest
}: TextFieldProps) {
  const generatedId = useId()
  const fieldId = id ?? generatedId
  const describedBy = error ? `${fieldId}-error` : hint ? `${fieldId}-hint` : undefined

  return (
    <div>
      <div className="mb-1.5 flex items-center justify-between gap-3 text-xs font-medium text-text-muted">
        <label htmlFor={fieldId}>{label}</label>
        {hint ? <span className="font-normal text-text-dim">{hint}</span> : null}
      </div>
      <div className="relative">
        <input
          id={fieldId}
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy}
          className={cn(
            'h-11 w-full rounded-xl border bg-black/25 px-3.5 text-sm text-text-primary outline-none transition',
            'placeholder:text-text-dim',
            'focus:border-neon-emerald/50 focus:bg-black/35 focus:ring-2 focus:ring-neon-emerald/15',
            error ? 'border-neon-rose/50' : 'border-white/10',
            trailing ? 'pr-11' : undefined,
            className,
          )}
          {...rest}
        />
        {trailing ? (
          <span className="absolute inset-y-0 right-3 flex items-center text-text-dim">
            {trailing}
          </span>
        ) : null}
      </div>
      {error ? (
        <span id={`${fieldId}-error`} className="mt-1.5 block text-xs text-neon-rose">
          {error}
        </span>
      ) : hint ? (
        <span id={`${fieldId}-hint`} className="mt-1.5 block text-xs text-text-dim">
          {hint}
        </span>
      ) : null}
    </div>
  )
}
