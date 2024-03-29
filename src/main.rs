use std::time::Duration;
use std::thread;

use libftd2xx::{Ftdi, FtdiCommon, BitMode};
use ftdi_mpsse::MpsseCmdBuilder;

//Instructions
const IDCODE:   u8 = 0x09;
const USER1:    u8 = 0x02;
const USER2:    u8 = 0x03;
const USER3:    u8 = 0x22;
const USER4:    u8 = 0x23;
const USERCODE: u8 = 0x08;

fn wait_data(ft: &mut Ftdi){
    loop {
        let queue_status = ft.queue_status().unwrap();

        if queue_status != 0 {break}

        let status = ft.status().unwrap();

        println!("Data wait: status: {:?}", status);
        println!("Data wait: looping: {}", queue_status);

        thread::sleep(Duration::from_millis(10));
    }
}

fn sync(ft: &mut Ftdi){
    //Send a bad command to sync
    let bad_command: [u8; 1] = [0xaa; 1];
    ft.write_all(&bad_command).unwrap();

    wait_data(ft);

    let mut buf: [u8; 2] = [0; 2];
    ft.read_all(&mut buf).unwrap();

    assert_eq!(buf[0], 0xfa);
    assert_eq!(buf[1], 0xaa);
}

//Ensure the TAP state machine is in the reset state
fn reset_tap(ft: &mut Ftdi){
    let cmd 
        = MpsseCmdBuilder::new()
        .clock_tms_out(ftdi_mpsse::ClockTMSOut::NegEdge, 0x7f, false, 7);

    ft.write_all(cmd.as_slice()).unwrap();
}


//Shift instruction
//Ends in the Exit IR state
fn shift_ir(ft: &mut Ftdi, insn: u8, len: u8){
    assert!(len >= 2);

    //Shift in the IR
    //IR length is 6 bits
    let cmd = MpsseCmdBuilder::new()
        .clock_bits_out(ftdi_mpsse::ClockBitsOut::LsbNeg, insn, len - 1);
    ft.write_all(cmd.as_slice()).unwrap();

    //Shift the final instruction bit
    //Transition to Exit IR (1)
    let cmd = MpsseCmdBuilder::new()
        .clock_tms_out(ftdi_mpsse::ClockTMSOut::NegEdge, 0x01, false, 1);
    ft.write_all(cmd.as_slice()).unwrap();
}

//Shift data register
//Ends in Exit DR
fn shift_dr(ft: &mut Ftdi, data: u8, len: u8){
    assert!(len >= 2);

    //Shift in the DR
    //8 Bits, all 0s
    let cmd = MpsseCmdBuilder::new()
        .clock_bits(ftdi_mpsse::ClockBits::LsbPosIn, data, 7);
    ft.write_all(cmd.as_slice()).unwrap();

    //Shift the final bit
    //Transition to Exit DR (1)
    //TODO: set last data bit
    let cmd = MpsseCmdBuilder::new()
        .clock_tms(ftdi_mpsse::ClockTMS::NegTMSPosTDO, 0x01, false, 1);
    ft.write_all(cmd.as_slice()).unwrap();
}

fn shift_bytes(ft: &mut Ftdi, data: &[u8]){
    assert!(data.len() >= 2);

    let (last, init) = data.split_last().unwrap();

    //Shift in the DR
    //8 Bits, all 0s
    let cmd = MpsseCmdBuilder::new()
        .clock_data(ftdi_mpsse::ClockData::LsbPosIn, init);
    ft.write_all(cmd.as_slice()).unwrap();

    //Shift in the DR
    let cmd = MpsseCmdBuilder::new()
        .clock_bits(ftdi_mpsse::ClockBits::LsbPosIn, *last, 7);
    ft.write_all(cmd.as_slice()).unwrap();

    //Shift the final bit
    //Transition to Exit DR (1)
    //TODO: set last data bit
    let cmd = MpsseCmdBuilder::new()
        .clock_tms(ftdi_mpsse::ClockTMS::NegTMSPosTDO, 0x01, false, 1);
    ft.write_all(cmd.as_slice()).unwrap();
}

fn reset_to_shift_dr(ft: &mut Ftdi) {
    //Get from reset to shift DR
    //Reset -0-> Idle -1-> DR scan -0-> Capture DR -0-> Shift DR
    let cmd = MpsseCmdBuilder::new()
        .clock_tms_out(ftdi_mpsse::ClockTMSOut::NegEdge, 0x2, false, 4);
    ft.write_all(cmd.as_slice()).unwrap();
}

fn reset_to_shift_ir(ft: &mut Ftdi){
    //Get from reset to shift IR
    //Reset -0-> Idle -1-> DR scan -1-> IR scan -0-> Capture IR -0-> Shift IR
    //Initial transition to reset seems unnecessary
    let cmd = MpsseCmdBuilder::new()
        .clock_tms_out(ftdi_mpsse::ClockTMSOut::NegEdge, 0x6, false, 5);
    ft.write_all(cmd.as_slice()).unwrap();
}

fn exit_ir_to_shift_dr(ft: &mut Ftdi){
    //Get to shift DR
    //Exit IR -1-> Update IR -1-> DR Scan -0-> Capture DR -0-> Shift DR
    let cmd = MpsseCmdBuilder::new()
        .clock_tms_out(ftdi_mpsse::ClockTMSOut::NegEdge, 0x03, false, 4);
    ft.write_all(cmd.as_slice()).unwrap();
}

fn exit_dr_to_reset(ft: &mut Ftdi){
    //Back to TAP reset
    //Exit DR -1-> Update DR -1-> Select DR -1-> Select IR -1-> Reset
    let cmd = MpsseCmdBuilder::new()
        .clock_tms_out(ftdi_mpsse::ClockTMSOut::NegEdge, 0xff, false, 4);
    ft.write_all(cmd.as_slice()).unwrap();
}

fn main() {
    let mut ft = Ftdi::new().unwrap();

    //Device and driver info
    let info = ft.device_info().unwrap();
    println!("Device information: {:?}", info);

    let drv = ft.driver_version().unwrap();
    println!("Driver version: {:?}", drv);

    //Reset
    ft.reset().unwrap();

    //Debug
    let status = ft.status().unwrap();
    println!("Status: {:?}", status);

    let queue_status = ft.queue_status().unwrap();
    println!("Queue status: {:?}", queue_status);

    //Setup incantation
    ft.set_usb_parameters(16384).unwrap();
    ft.set_chars(0, false, 0, false).unwrap();
    ft.set_timeouts(Duration::from_millis(5000), Duration::from_millis(1000)).unwrap();
    ft.set_latency_timer(Duration::from_millis(16)).unwrap();

    //Reset and enable MPSSE mode
    ft.set_bit_mode(0, BitMode::Reset).unwrap();
    ft.set_bit_mode(0, BitMode::Mpsse).unwrap();

    sync(&mut ft);

    //JTAG setup
    let cmd = MpsseCmdBuilder::new()
        .set_clock(0x5db, Some(false))
        .disable_adaptive_data_clocking()
        .disable_3phase_data_clocking()
        .disable_loopback();

    println!("{:x?}", cmd.as_slice());
    ft.write_all(cmd.as_slice()).unwrap();

    //Port direction and initial values
    let cmd = MpsseCmdBuilder::new()
        .set_gpio_lower(0x08, 0x0b)
        .set_gpio_upper(0x00, 0x00);

    println!("{:x?}", cmd.as_slice());
    ft.write_all(cmd.as_slice()).unwrap();

    reset_tap(&mut ft);

    reset_to_shift_ir(&mut ft);
    //reset_to_shift_dr(&mut ft);

    shift_ir(&mut ft, IDCODE, 6);

    exit_ir_to_shift_dr(&mut ft);

    //shift_dr(&mut ft, 0, 8);
    shift_bytes(&mut ft, &[0, 0, 0, 0]);

    exit_dr_to_reset(&mut ft);

    //Read back
    wait_data(&mut ft);

    let mut buf: [u8; 5] = [0; 5];
    ft.read_all(&mut buf).unwrap();

    println!("{:x?}", buf);
}
