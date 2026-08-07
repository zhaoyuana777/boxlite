/*
 * Copyright 2025 BoxLite AI
 * SPDX-License-Identifier: AGPL-3.0
 */

import { TrackJobExecution } from './track-job-execution.decorator'

const deferred = () => {
  let resolve!: () => void
  const promise = new Promise<void>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

class ConcurrentJobs {
  activeJobs = new Set<symbol>()

  @TrackJobExecution()
  async run(completion: Promise<void>) {
    await completion
  }
}

describe('TrackJobExecution', () => {
  it('tracks concurrent invocations of the same method independently', async () => {
    const jobs = new ConcurrentJobs()
    const first = deferred()
    const second = deferred()

    const firstRun = jobs.run(first.promise)
    const secondRun = jobs.run(second.promise)
    expect(jobs.activeJobs.size).toBe(2)

    first.resolve()
    await firstRun
    expect(jobs.activeJobs.size).toBe(1)

    second.resolve()
    await secondRun
    expect(jobs.activeJobs.size).toBe(0)
  })
})
