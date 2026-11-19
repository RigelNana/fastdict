import test from 'node:test'
import assert from 'node:assert/strict'
import { uploadChunks } from '../src/lib/chunk-upload.ts'

const files = [new File(['abcdefghij'], 'folder/dictionary.mdd'), new File(['xyz'], 'style.css')]

test('chunks preserve relative paths and total progress; a lost response retries only that same block', async () => {
  const writes = [], progress = [], committed = new Map()
  let lost = false, completed = 0, discarded = 0
  const result = await uploadChunks(files, {
    async create(spec) {
      assert.deepEqual(spec, [{path:'folder/dictionary.mdd',size:10},{path:'style.css',size:3}])
      return {id:'session',chunk_size:4}
    },
    async write(id,index,offset,body,onProgress) {
      const text = await body.text();writes.push([index,offset,text]);onProgress(body.size)
      const key = `${index}:${offset}`
      assert.ok(!committed.has(key) || committed.get(key) === text)
      committed.set(key,text)
      if (!lost && offset === 4) { lost = true; throw {status:0} }
      return {received:offset+body.size}
    },
    async complete() { completed++; if (completed === 1) throw {status:502}; return {revision:'done'} },
    async discard() { discarded++ },
  }, state => progress.push(state), async () => {})
  assert.deepEqual(result,{revision:'done'})
  assert.deepEqual(writes,[[0,0,'abcd'],[0,4,'efgh'],[0,4,'efgh'],[0,8,'ij'],[1,0,'xyz']])
  assert.equal(completed,2)
  assert.equal(discarded,1)
  assert.ok(progress.some(p=>p.retry===1))
  assert.ok(progress.every((p,i)=>p.total===13 && (i===0 || p.loaded>=progress[i-1].loaded)))
  assert.deepEqual(progress.at(-1),{phase:'processing',loaded:13,total:13,retry:1})
})

test('rejected chunks do not publish files, and retries are bounded', async () => {
  for (const [status, expected] of [[413,1],[422,1],[0,3],[503,3]]) {
    let tries=0,completed=false,discarded=false
    await assert.rejects(uploadChunks(files,{
      async create(){return{id:'session',chunk_size:4}},
      async write(){tries++;throw Object.assign(new Error('failed'),{status})},
      async complete(){completed=true},async discard(){discarded=true},
    },()=>{},async()=>{}),/failed/)
    assert.equal(tries,expected)
    assert.equal(completed,false)
    assert.equal(discarded,true)
  }
})

test('large-file validation precedes creating server staging, and zero-byte files are completed',async()=>{
  let created=false
  for (const sizes of [[50*1024**3+1], [25*1024**3,25*1024**3+1]]) {
    await assert.rejects(uploadChunks(sizes.map((size,i)=>({name:`huge.${i}.mdd`,size})),{async create(){created=true}},()=>{}),/50 GiB/)
  }
  assert.equal(created,false)
  let writes=0
  const result=await uploadChunks([new File([],'empty.css')],{
    async create(){return{id:'empty',chunk_size:8}},async write(){writes++},async complete(){return'done'},async discard(){},
  },()=>{})
  assert.equal(result,'done')
  assert.equal(writes,0)
})

test('50 GiB batches retain exact offsets and progress beyond 32-bit boundaries', async () => {
  // Virtual file slices exercise the real transfer loop without allocating 50 GiB.
  const sizes = [3*1024**3,47*1024**3]
  const received = [0,0]
  const progress = []
  const result = await uploadChunks(sizes.map((size,i)=>({
    name:i===0?'dictionary.mdx':'dictionary.mdd',size,
    slice(start,end){return {size:end-start}},
  })), {
    async create(spec){assert.equal(spec.reduce((sum,f)=>sum+f.size,0),50*1024**3);return{id:'large',chunk_size:8*1024**2}},
    async write(id,index,offset,body,onProgress){
      assert.equal(offset,received[index]);assert.ok(body.size<=8*1024**2)
      received[index]+=body.size;onProgress(body.size);return{received:received[index]}
    },
    async complete(){return'done'},async discard(){},
  },state=>progress.push(state))
  assert.equal(result,'done')
  assert.deepEqual(received,sizes)
  assert.ok(progress.every((p,i)=>Number.isSafeInteger(p.loaded) && (i===0 || p.loaded>=progress[i-1].loaded)))
  assert.deepEqual(progress.at(-1),{phase:'processing',loaded:50*1024**3,total:50*1024**3})
})
