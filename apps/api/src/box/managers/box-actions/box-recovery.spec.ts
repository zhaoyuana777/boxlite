import { BoxStartAction } from './box-start.action'
import { Box } from '../../entities/box.entity'
import { BoxState } from '../../enums/box-state.enum'
import { BoxDesiredState } from '../../enums/box-desired-state.enum'
import { RunnerState } from '../../enums/runner-state.enum'
import { LockCode } from '../../common/redis-lock.provider'

function fixture(state = BoxState.STOPPED) {
  const box = Object.assign(new Box('region', 'original'), {
    state,
    desiredState: BoxDesiredState.STARTED,
    pending: true,
    runnerId: 'runner',
    authToken: 'TEST_BOX_AUTH_TOKEN',
    image: 'original-image',
    errorReason: 'previous failure',
    recoveryStartedAt: new Date(),
  })
  const adapter = {
    recoverBox: jest.fn().mockResolvedValue(undefined),
    startBox: jest.fn(),
    createBox: jest.fn(),
    boxInfo: jest.fn().mockResolvedValue({ state: BoxState.STARTED }),
  }
  const lock = new LockCode('recovery-lock')
  const getCode = jest.fn().mockResolvedValue(lock)
  const repository = {
    update: jest.fn(async (_id, { updateData }) => {
      Object.assign(box, updateData)
      box.enforceInvariants()
      return box
    }),
  }
  const action = new BoxStartAction(
    { findOneOrFail: jest.fn().mockResolvedValue({ apiVersion: '0', state: RunnerState.READY }) } as any,
    { create: jest.fn().mockResolvedValue(adapter) } as any,
    repository as any,
    { findOne: jest.fn().mockResolvedValue({}) } as any,
    {} as any,
    { getCode } as any,
    { getLastActivityAt: jest.fn().mockResolvedValue(new Date()) } as any,
  )
  return { box, adapter, action, lock, getCode }
}

describe('Box recovery', () => {
  it('dispatches recovery once and waits for the original VM before clearing the error', async () => {
    const { box, adapter, action, lock } = fixture()
    await action.run(box, lock)
    expect(adapter.recoverBox).toHaveBeenCalledWith(box)
    expect(adapter.startBox).not.toHaveBeenCalled()
    expect(box.state).toBe(BoxState.STARTING)
    expect(box.errorReason).toBe('previous failure')
    await action.run(box, lock)
    expect(box.state).toBe(BoxState.STARTED)
    expect(box.recoveryStartedAt).toBeNull()
    expect(box.errorReason).toBeNull()
    expect(adapter.recoverBox).toHaveBeenCalledTimes(1)
  })

  it.each([BoxState.UNKNOWN, BoxState.DESTROYED, BoxState.ERROR, BoxState.STOPPED])(
    'fails safely when the original VM reports %s after a process restart',
    async (runtimeState) => {
      const { box, adapter, action, lock } = fixture(BoxState.STARTING)
      // The only recovery context is the persisted entity, not a request-local flag.
      adapter.boxInfo.mockResolvedValue({ state: runtimeState })
      await action.run(box, lock)
      expect(box.state).toBe(BoxState.ERROR)
      expect(box.errorReason).toMatch(/recover/i)
      expect(box.recoveryStartedAt).toBeNull()
      expect(adapter.createBox).not.toHaveBeenCalled()
      expect(adapter.startBox).not.toHaveBeenCalled()
    },
  )

  it('never creates a replacement when the persisted recovery state is UNKNOWN', async () => {
    const { box, adapter, action, lock } = fixture(BoxState.UNKNOWN)
    adapter.boxInfo.mockResolvedValue({ state: BoxState.UNKNOWN })
    await action.run(box, lock)
    expect(adapter.createBox).not.toHaveBeenCalled()
    expect(box.state).toBe(BoxState.ERROR)
  })

  it('does not dispatch recovery after losing the lifecycle lock', async () => {
    const { box, adapter, action, lock, getCode } = fixture()
    getCode.mockResolvedValue(null)
    await action.run(box, lock)
    expect(adapter.recoverBox).not.toHaveBeenCalled()
    expect(adapter.startBox).not.toHaveBeenCalled()
    expect(box.state).toBe(BoxState.STOPPED)
  })

  it.each([
    ['disk image not found', /backup/],
    ['permission denied', /permissions/],
    ['invalid qcow header', /offline/],
    ['recover: stop original VM: process busy', /process and disk lock/],
    ['recover: auto-delete', /automatic deletion/],
    ['unexpected failure credential=PRIVATE_TEST_VALUE', /runner logs/],
  ])('reports safe guidance for %s', async (message, guidance) => {
    const { box, adapter, action, lock } = fixture()
    adapter.recoverBox.mockRejectedValue(new Error(message))
    await action.run(box, lock)
    expect(box.state).toBe(BoxState.ERROR)
    expect(box.errorReason).toMatch(guidance)
    expect(box.errorReason).not.toContain('PRIVATE_TEST_VALUE')
    expect(box.recoveryStartedAt).toBeNull()
  })

  it('terminates recovery after the persisted deadline, even after an API restart', async () => {
    const { box, adapter, action, lock } = fixture(BoxState.STARTING)
    box.recoveryStartedAt = new Date(Date.now() - 6 * 60_000)
    adapter.boxInfo.mockResolvedValue({ state: BoxState.STARTING })
    await action.run(box, lock)
    expect(box.state).toBe(BoxState.ERROR)
    expect(box.errorReason).toMatch(/timed out|timeout/i)
    expect(box.pending).toBe(false)
  })

  it('persists actionable storage failure without exposing arbitrary error details', async () => {
    const { box, adapter, action, lock } = fixture()
    adapter.recoverBox.mockRejectedValue(new Error('no space left on device; credential=PRIVATE_TEST_VALUE'))
    await action.run(box, lock)
    expect(box.state).toBe(BoxState.ERROR)
    expect(box.errorReason).toMatch(/space|quota/i)
    expect(box.errorReason).toMatch(/administrator|capacity/i)
    expect(box.errorReason).not.toContain('PRIVATE_TEST_VALUE')
    expect(box.recoverable).toBe(true)
    expect(box.pending).toBe(false)
  })
})
