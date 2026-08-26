/*
 * Copyright 2026 BoxLite AI
 * SPDX-License-Identifier: AGPL-3.0
 */

import { randomUUID } from 'node:crypto'
import { DataSource, Repository } from 'typeorm'
import { CustomNamingStrategy } from '../../common/utils/naming-strategy.util'
import { Box } from '../entities/box.entity'
import { BoxLastActivity } from '../entities/box-last-activity.entity'
import { BoxDesiredState } from '../enums/box-desired-state.enum'
import { BoxState } from '../enums/box-state.enum'
import { BoxRepository } from './box.repository'

const describeIfDatabase = process.env.DB_HOST ? describe : describe.skip
const schemaName = `box_admin_read_${process.pid}_${randomUUID().replaceAll('-', '')}`
const organizationId = '00000000-0000-4000-8000-0000000000ff'

function adminBox(id: string, name: string): Box {
  const box = new Box('us', name)
  box.id = id
  box.organizationId = organizationId
  box.osUser = 'boxlite'
  box.state = BoxState.STOPPED
  box.desiredState = BoxDesiredState.STOPPED
  box.pending = false
  return box
}

describeIfDatabase('BoxLite admin read pagination (integration, real Postgres)', () => {
  let dataSource: DataSource
  let boxes: Repository<Box>
  let repository: BoxRepository
  let ownsSchema = false

  beforeAll(async () => {
    dataSource = await new DataSource({
      type: 'postgres',
      host: process.env.DB_HOST,
      port: Number(process.env.DB_PORT || 5432),
      username: process.env.DB_USERNAME,
      password: process.env.DB_PASSWORD,
      database: process.env.DB_DATABASE,
      schema: schemaName,
      entities: [Box, BoxLastActivity],
      namingStrategy: new CustomNamingStrategy(),
      entitySkipConstructor: true,
      synchronize: false,
      extra: { options: `-c search_path=${schemaName},public` },
    }).initialize()

    await dataSource.query(`CREATE SCHEMA "${schemaName}"`)
    ownsSchema = true
    await dataSource.synchronize()
    boxes = dataSource.getRepository(Box)
    repository = new BoxRepository(
      dataSource,
      { emit: jest.fn() } as any,
      {
        invalidate: jest.fn(),
        invalidateOrgId: jest.fn(),
      } as any,
    )
  })

  afterAll(async () => {
    if (!dataSource?.isInitialized) return

    try {
      if (ownsSchema) await dataSource.query(`DROP SCHEMA IF EXISTS "${schemaName}" CASCADE`)
    } finally {
      await dataSource.destroy()
    }
  })

  it('does not omit rows whose updatedAt values differ only below JavaScript millisecond precision', async () => {
    const rows = [
      { box: adminBox('Aa0000000001', 'newest'), updatedAt: '2026-08-26T00:00:00.123900Z' },
      { box: adminBox('Aa0000000002', 'middle'), updatedAt: '2026-08-26T00:00:00.123800Z' },
      { box: adminBox('Aa0000000003', 'oldest'), updatedAt: '2026-08-26T00:00:00.123700Z' },
    ]
    await boxes.insert(rows.map(({ box }) => box))
    for (const row of rows) {
      await dataSource.query(`UPDATE "${schemaName}"."box" SET "updatedAt" = $1::timestamptz WHERE "id" = $2`, [
        row.updatedAt,
        row.box.id,
      ])
    }

    const firstPage = await repository.findAdminPage({ limit: 2, filters: {} })
    const boundary = firstPage.items.at(-1)
    if (!boundary) throw new Error('Expected a first-page boundary')

    const secondPage = await repository.findAdminPage({
      limit: 2,
      filters: {},
      after: { updatedAt: boundary.cursorUpdatedAt, id: boundary.id },
    })

    expect(firstPage.hasMore).toBe(true)
    expect(boundary.cursorUpdatedAt).toBe(rows[1].updatedAt)
    expect([...firstPage.items, ...secondPage.items].map(({ id }) => id)).toEqual(rows.map(({ box }) => box.id))
  })
})
