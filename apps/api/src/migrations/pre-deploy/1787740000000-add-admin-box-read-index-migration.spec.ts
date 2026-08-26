import { AddAdminBoxReadIndex1787740000000 } from './1787740000000-add-admin-box-read-index-migration'

describe('AddAdminBoxReadIndex1787740000000', () => {
  it('BoxLite admin read creates the descending keyset pagination index', async () => {
    const query = jest.fn()
    const migration = new AddAdminBoxReadIndex1787740000000()

    await migration.up({ query } as any)

    expect(query).toHaveBeenCalledWith(`CREATE INDEX "box_updatedat_id_idx" ON "box" ("updatedAt" DESC, "id" DESC)`)
  })
})
