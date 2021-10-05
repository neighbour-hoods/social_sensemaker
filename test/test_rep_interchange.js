const path = require('path')
const tape = require('tape')

const { Orchestrator, Config, combine, tapeExecutor, localOnly } = require('@holochain/tryorama')


//helpers

const buildRunner = () => new Orchestrator({
  middleware: combine(
    tapeExecutor(tape),
    localOnly,
  ),
})

// DNA loader, to be used with `buildTestScenario` when constructing DNAs for testing
const getDNA = ((dnas) => (name) => (dnas[name]))({
  'rep_interchange': path.resolve(__dirname, '../happs/rep_interchange/rep_dsl_test_dna.dna'),
})

// temporary method for RSM until conductor can interpret consistency
function shimConsistency (s) {
  s.consistency = () => new Promise((resolve, reject) => {
    setTimeout(resolve, 100)
  })
}




// main test script

const runner = buildRunner()
const config = Config.gen()

runner.registerScenario('Basic DSL program compilation', async (scenario, t) => {
  shimConsistency(scenario)

  const [player] = await scenario.players([config])
  const [[firstHapp]] = await player.installAgentsHapps([
    [ // agent 1
      [  // hApp bundle 1
        getDNA('rep_interchange'),  // composed of these DNAs
      ]
    ],
  ])
//  const appCellIds = firstHapp.cells.map(c => c.cellNick.match(/(\w+)\.dna$/)[1])

  const repInterchangeApp = firstHapp.cells[0]

  const resultGood = await repInterchangeApp.call('interpreter', 'test_output', { params_string: "(lam [x] (if x 1 2))" })
  await scenario.consistency()
  console.log('good call', resultGood)
  t.equal(resultGood, true)

  // the commonalities here can be abstracted
  const resultBad = await repInterchangeApp.call('interpreter', 'test_output', { params_string: "$$$$$!" })
  await scenario.consistency()
  console.log('bad call:', resultBad)
  t.equal(resultBad, false)

  const expr = "(lam [x] (if x 1 2))"
  const resultExpr = await repInterchangeApp.call('interpreter', 'create_interchange_entry_parse', { expr: expr, args: [] })
  await scenario.consistency()
  console.log('header hash: ', resultExpr)
  t.equal(resultExpr, false)
  const resultIE = await repInterchangeApp.call('interpreter', 'get_interchange_entry', resultExpr)
  await scenario.consistency()
  t.equal(resultIE.operator, expr)
})

runner.run()
