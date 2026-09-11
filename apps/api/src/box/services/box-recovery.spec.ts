import { BoxService } from './box.service'
import { Box } from '../entities/box.entity'
import { BoxState } from '../enums/box-state.enum'
import { BoxDesiredState } from '../enums/box-desired-state.enum'

describe('BoxService recovery intent', () => {
  function fixture() {
    const box = Object.assign(new Box('region', 'original'), {
      state: BoxState.ERROR,
      pending: false,
      runnerId: 'runner',
      authToken: 'TEST_BOX_AUTH_TOKEN',
      errorReason: 'original failure',
    })
    const adapter = { recoverBox: jest.fn() }
    const service = Object.create(BoxService.prototype) as BoxService
    const updateWhere = jest.fn(async (_id, { updateData }) => {
      Object.assign(box, updateData)
      box.enforceInvariants()
      return box
    })
    const emit = jest.fn()
    Object.assign(service, {
      findOneByIdOrName: jest.fn().mockResolvedValue(box),
      runnerService: { findOneOrFail: jest.fn().mockResolvedValue({ apiVersion: '0' }) },
      runnerAdapterFactory: { create: jest.fn().mockResolvedValue(adapter) },
      organizationService: { assertOrganizationIsNotSuspended: jest.fn() },
      boxRepository: { updateWhere },
      eventEmitter: { emit },
      start: jest.fn().mockResolvedValue(box),
    })
    return { service, box, adapter, updateWhere, emit }
  }

  it('durably claims recovery before emitting work and preserves credentials/configuration', async () => {
    const { service, box, adapter, updateWhere, emit } = fixture()
    const result = await service.recover(box.id, { id: 'org' } as any)
    expect(updateWhere).toHaveBeenCalledWith(
      box.id,
      expect.objectContaining({
        whereCondition: { state: BoxState.ERROR, pending: false },
        updateData: expect.objectContaining({
          recoveryStartedAt: expect.any(Date),
          desiredState: BoxDesiredState.STARTED,
        }),
      }),
    )
    expect(result.pending).toBe(true)
    expect(result.errorReason).toBe('original failure')
    expect(result.authToken).toBe('TEST_BOX_AUTH_TOKEN')
    expect(adapter.recoverBox).not.toHaveBeenCalled()
    expect(service.start).not.toHaveBeenCalled()
    expect(emit).toHaveBeenCalledTimes(1)
  })

  it('does not dispatch when another lifecycle operation wins the conditional update', async () => {
    const { service, box, adapter, updateWhere, emit } = fixture()
    updateWhere.mockRejectedValue(new Error('conflict'))
    await expect(service.recover(box.id, { id: 'org' } as any)).rejects.toThrow('conflict')
    expect(adapter.recoverBox).not.toHaveBeenCalled()
    expect(emit).not.toHaveBeenCalled()
  })

  it('ignores a stale heartbeat while recovery owns the lifecycle', async () => {
    const { service, box, updateWhere } = fixture()
    Object.assign(box, { state: BoxState.STOPPED, recoveryStartedAt: new Date() })
    Object.assign(service, { boxRepository: { findOne: jest.fn().mockResolvedValue(box), updateWhere } })
    await service.updateState(box.id, BoxState.STARTED)
    expect(updateWhere).not.toHaveBeenCalled()
    expect(box.state).toBe(BoxState.STOPPED)
  })

  it('keeps recovery disabled for v2 runners', async () => {
    const { service, box, adapter, updateWhere } = fixture()
    Object.assign(service, { runnerService: { findOneOrFail: jest.fn().mockResolvedValue({ apiVersion: '2' }) } })
    await expect(service.recover(box.id, { id: 'org' } as any)).rejects.toThrow('not supported')
    expect(updateWhere).not.toHaveBeenCalled()
    expect(adapter.recoverBox).not.toHaveBeenCalled()
  })
})
