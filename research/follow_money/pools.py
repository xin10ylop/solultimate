import numpy as np, os
PO = np.dtype([('kind','u1'),('pad','u1'),('ev','<u2'),('slot','<u4'),('tx','<u4'),('pool','<u4'),('mint','<u4'),('user','<u4'),
               ('signer','<u4'),('fee_bps','<u4'),('base','<u8'),('quote','<u8'),('base_res','<u8'),('quote_res','<u8'),('vq','<i8'),('fee','<u8'),('extra','<u8'),('qmint','<u4')])
KIND = ['create','buy','sell','boostburn','initboost']
def load_pools(d):
    return np.fromfile(os.path.join(d,'pools.bin'),dtype=PO)
