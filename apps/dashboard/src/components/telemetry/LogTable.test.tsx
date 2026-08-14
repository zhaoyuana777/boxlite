// @vitest-environment jsdom
/*
 * Copyright Daytona Platforms Inc.
 * SPDX-License-Identifier: AGPL-3.0
 */

import { act } from 'react'
import { createRoot, Root } from 'react-dom/client'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { LogTable } from './LogTable'

describe('LogTable', () => {
  let root: Root | null = null

  beforeAll(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true
  })

  afterEach(() => {
    act(() => root?.unmount())
    root = null
    document.body.innerHTML = ''
  })

  it('exposes log expansion as a keyboard-operable button', () => {
    const host = document.createElement('div')
    document.body.appendChild(host)
    root = createRoot(host)

    act(() =>
      root?.render(
        <LogTable
          logs={[
            {
              timestamp: '2026-08-14T00:00:00.000Z',
              body: 'full log message',
              severityText: 'INFO',
              serviceName: 'boxlite-api',
              resourceAttributes: {},
              logAttributes: {},
            },
          ]}
          isLoading={false}
          isError={false}
          onRetry={vi.fn()}
        />,
      ),
    )

    const toggle = document.querySelector<HTMLButtonElement>('button[aria-label="Toggle log details"]')
    expect(toggle?.getAttribute('aria-expanded')).toBe('false')

    act(() => toggle?.click())

    expect(toggle?.getAttribute('aria-expanded')).toBe('true')
    expect(document.body.textContent).toContain('Full Message')
  })
})
