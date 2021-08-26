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

  const result = await repInterchangeApp.call('interpreter', 'test_output', { param: "thing" })
  await scenario.consistency()
  console.log('did a call!', result)

  t.equal(result, true)
})

runner.run()
