use rand::Rng;

pub fn toss_coin(probability: f32) -> bool {
    let mut rng = rand::thread_rng();
    let result: f32 = rng.gen();
    result < probability
}
